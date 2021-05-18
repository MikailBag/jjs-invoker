//! Actually implements request processing logic
use crate::toolchain::{PulledToolchain, ToolchainPuller};
use anyhow::Context as _;
use invoker_api::{
    invoke::{
        Action, Command, EnvVarValue, EnvironmentVariable, Extensions, InputSource, InvokeRequest,
        PathPrefix, PrefixedPath, SandboxSettings,
    },
    shim::{
        RequestExtensions, SandboxSettingsExtensions, SharedDirExtensionSource,
        EXTRA_FILES_DIR_NAME,
    },
};
#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
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
        let contents = load_input(&v.contents)
            .await
            .with_context(|| format!("failed to fetch input {}", k))?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(&parent).await.with_context(|| {
                format!(
                    "failed to create parent directory for extraFile: {}",
                    path.display()
                )
            })?;
        }
        tokio::fs::write(&path, contents)
            .await
            .with_context(|| format!("failed to prepare extraFile {}", path.display()))?;
        if v.executable {
            let mut m = tokio::fs::metadata(&path)
                .await
                .with_context(|| format!("failed to read current metadata of {}", path.display()))?
                .permissions();
            m.set_mode(m.mode() | 0o111);
            tokio::fs::set_permissions(&path, m)
                .await
                .with_context(|| format!("failed to update permissions for {}", path.display()))?;
        }
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
    let request_shared_dir: PathBuf =
        format!("/var/invoker/work/{}/exchange", req.id.to_hyphenated()).into();

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
            rewrite_prefixed_path(path, &invoker_extra_files_dir)?;
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

fn make_relative(path: &Path) -> PathBuf {
    if path.has_root() {
        let mut iter = path.components().peekable();

        loop {
            match iter.peek() {
                Some(Component::Prefix(_)) => iter.next(),
                Some(Component::RootDir) => iter.next(),
                _ => break,
            };
        }

        iter.collect()
    } else {
        path.to_path_buf()
    }
}

fn rewrite_prefixed_path(path: &mut PrefixedPath, extra_files_dir: &Path) -> anyhow::Result<()> {
    if let PathPrefix::Extension(ext) = &mut path.prefix {
        let ext: SharedDirExtensionSource = serde_json::from_value(take_ext(ext))?;
        if ext.name == EXTRA_FILES_DIR_NAME {
            path.prefix = PathPrefix::Host;
            path.path = make_relative(&extra_files_dir.join(&path.path));
        } else {
            anyhow::bail!("unknown prefix name: {}", ext.name);
        }
    }
    Ok(())
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
        rewrite_prefixed_path(&mut shared_dir.host_path, invoker_extra_files_dir)?;
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::make_relative;
    #[test]
    fn test_make_relative() {
        assert_eq!(make_relative(Path::new("/foo/bar")), Path::new("foo/bar"));
        assert_eq!(
            make_relative(Path::new("hi/there.txt")),
            Path::new("hi/there.txt")
        );
    }
}
