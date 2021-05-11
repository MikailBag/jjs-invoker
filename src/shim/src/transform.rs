//! Actually implements request processing logic
use crate::toolchain::{PulledToolchain, ToolchainPuller};
use anyhow::Context as _;
use invoker_api::{
    invoke::{
        Action, Command, EnvVarValue, EnvironmentVariable, Extensions, InputSource, InvokeRequest,
        SandboxSettings,
    },
    shim::{RequestExtensions, SandboxSettingsExtensions, EXTRA_FILES_DIR_NAME, WORK_DIR_NAME},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

fn take_ext(ext: &mut Extensions) -> serde_json::Value {
    serde_json::Value::Object(std::mem::take(&mut ext.0))
}

async fn load_input(input: &InputSource) -> anyhow::Result<Vec<u8>> {
    match input {
        InputSource::LocalFile { path } => tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read {}", path.display())),
        InputSource::InlineString { data } => Ok(data.as_bytes().to_vec()),
        InputSource::InlineBase64 { data } => base64::decode(data).context("invalid base64"),
    }
}

pub(crate) async fn transform_request(
    req: &mut InvokeRequest,
    toolchain_puller: Arc<ToolchainPuller>,
    local_exchange_dir: &Path,
    invoker_exchange_dir: &Path,
) -> anyhow::Result<()> {
    let exts: RequestExtensions =
        serde_json::from_value(take_ext(&mut req.ext)).context("invalid request extensions")?;

    let local_extra_files_dir = local_exchange_dir
        .join("extra")
        .join(req.id.to_hyphenated().to_string());

    tokio::fs::create_dir_all(&local_extra_files_dir)
        .await
        .with_context(|| {
            format!(
                "failed to create directory {} for storing extraFiles",
                local_extra_files_dir.display()
            )
        })?;

    let invoker_extra_files_dir = invoker_exchange_dir
        .join("extra")
        .join(req.id.to_hyphenated().to_string());

    for (k, v) in &exts.extra_files {
        if k.starts_with('/') || k.starts_with('\\') {
            anyhow::bail!("extraFiles.map specifies absolute path {}", k);
        }
        let path = local_extra_files_dir.join(k);
        let v = load_input(v)
            .await
            .with_context(|| format!("failed to fetch input {}", k))?;
        tokio::fs::write(&path, v)
            .await
            .with_context(|| format!("failed to prepare extraFile {}", path.display()))?;
    }

    let dict = crate::interp::get_interpolation_dict(&exts)
        .context("failed to prepare interpolation context")?;

    let mut tcx = TooclhainsUtil {
        invoker_exchange_dir: invoker_exchange_dir.to_path_buf(),
        puller: toolchain_puller,
        sandbox_images: HashMap::new(),
        toolchains: HashMap::new(),
    };

    // TODO: allow customization here
    let request_shared_dir: PathBuf = format!("/tmp/{}", req.id.to_hyphenated()).into();

    // at first, we transform sandboxes
    for step in &mut req.steps {
        let action = &mut step.action;
        if let Action::CreateSandbox(sb) = action {
            transform_sandbox(sb, &invoker_extra_files_dir, &request_shared_dir, &mut tcx).await?;
        }
    }

    // now, we transform commands
    for step in &mut req.steps {
        let action = &mut step.action;
        if let Action::ExecuteCommand(cmd) = action {
            transform_command(cmd, &tcx, &dict).await?;
        }
    }

    // finally, let's rewrite paths in file manipulation actions
    for step in &mut req.steps {
        let action = &mut step.action;
        if let Action::OpenFile { path, .. } = action {
            *path = rewrite_path(path, &invoker_extra_files_dir, &request_shared_dir);
        }
    }

    Ok(())
}

struct TooclhainsUtil {
    puller: Arc<ToolchainPuller>,
    toolchains: HashMap<String, PulledToolchain>,
    sandbox_images: HashMap<String, String>,
    invoker_exchange_dir: PathBuf,
}

impl TooclhainsUtil {
    fn update_command(&self, cmd: &mut Command) -> anyhow::Result<()> {
        let image = self
            .sandbox_images
            .get(&cmd.sandbox_name)
            .with_context(|| format!("command references unknown sandbox {}", cmd.sandbox_name))?;
        let pulled = &self.toolchains[image];
        let mut used_names = HashSet::new();
        for var in &cmd.env {
            used_names.insert(var.name.clone());
        }
        for (k, v) in pulled.get_env() {
            if !used_names.contains(&k) {
                cmd.env.push(EnvironmentVariable {
                    name: k.clone(),
                    value: EnvVarValue::Plain(v.clone()),
                    ext: Default::default(),
                });
            }
        }
        Ok(())
    }

    async fn pull_if_needed(&mut self, image: &str, sandbox_name: &str) -> anyhow::Result<PathBuf> {
        self.sandbox_images
            .insert(sandbox_name.to_string(), image.to_string());
        let pulled = if let Some(cached) = self.toolchains.get(image) {
            cached.clone()
        } else {
            tracing::info!(image_name = image, "Pulling toolchain");
            let pulled = self
                .puller
                .resolve(&image)
                .await
                .context("failed to pull image")?;
            self.toolchains.insert(image.to_string(), pulled.clone());
            pulled
        };
        Ok(self
            .invoker_exchange_dir
            .join("toolchains")
            .join(&pulled.path))
    }
}

fn path_starts_with<'a>(path: &'a Path, prefix: &str) -> Option<&'a Path> {
    let mut iter = path.components();
    let first = iter.next()?;
    if first.as_os_str() != prefix {
        return None;
    }
    return Some(iter.as_path());
}

fn rewrite_path(path: &Path, extra_files_dir: &Path, request_shared_dir: &Path) -> PathBuf {
    if let Some(suf) = path_starts_with(&path, EXTRA_FILES_DIR_NAME) {
        return extra_files_dir.join(suf);
    }
    if let Some(suf) = path_starts_with(&path, WORK_DIR_NAME) {
        return request_shared_dir.join(suf);
    }
    path.to_path_buf()
}

#[tracing::instrument(skip(sandbox, invoker_extra_files_dir, tcx), fields(sandbox_name = sandbox.name.as_str()))]
async fn transform_sandbox(
    sandbox: &mut SandboxSettings,
    invoker_extra_files_dir: &Path,
    request_shared_dir: &Path,
    tcx: &mut TooclhainsUtil,
) -> anyhow::Result<()> {
    let exts: SandboxSettingsExtensions = serde_json::from_value(take_ext(&mut sandbox.ext))
        .context("invalid sandbox settings extensions")?;
    if sandbox.base_image != Path::new("") {
        anyhow::bail!("baseImage must be empty");
    }
    sandbox.base_image = tcx.pull_if_needed(&exts.image, &sandbox.name).await?;

    for shared_dir in &mut sandbox.expose {
        if path_starts_with(&shared_dir.host_path, WORK_DIR_NAME).is_some() {
            shared_dir.create = true;
        }
        shared_dir.host_path = rewrite_path(
            &shared_dir.host_path,
            invoker_extra_files_dir,
            request_shared_dir,
        );
    }
    Ok(())
}

async fn transform_command(
    command: &mut Command,
    tcx: &TooclhainsUtil,
    interp_dict: &HashMap<String, String>,
) -> anyhow::Result<()> {
    *command = crate::interp::interpolate_command(command, interp_dict)?;
    tcx.update_command(command)?;

    Ok(())
}
