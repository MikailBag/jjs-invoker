use crate::{executor::path_resolver::PathResolver, interactive_debug::Suspender};
use anyhow::Context as _;
use invoker_api::{
    debug::AttachRequest,
    invoke::{SandboxSettings, SharedDirectoryMode},
};
use minion::{SharedItem, SharedItemKind};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::fs;

pub struct Sandbox {
    sandbox: Arc<dyn minion::erased::Sandbox>,
}

pub struct SandboxGlobalSettings {
    pub exposed_host_items: Option<Vec<String>>,
    pub skip_system_checks: bool,
    pub override_id_range: Option<(u32, u32)>,
    pub leak: bool,
    pub allow_fallback_pid_limit: bool,
    pub suspender: Arc<Suspender>,
}

fn default_process_limit(s: &SandboxGlobalSettings) -> u64 {
    if s.allow_fallback_pid_limit {
        // fallback implementation only supports 1 process per sandbox
        1
    } else {
        // otherwise let's apply limit that should be sufficient
        16
    }
}

impl Sandbox {
    pub fn raw_sandbox(&self) -> Arc<dyn minion::erased::Sandbox> {
        self.sandbox.clone()
    }

    pub async fn create(
        sandbox_data_dir: &Path,
        backend: &dyn minion::erased::Backend,
        settings: &SandboxSettings,
        global_settings: &SandboxGlobalSettings,
        path_resolver: &PathResolver,
        request_id: uuid::Uuid,
    ) -> anyhow::Result<Self> {
        let mut shared_items = vec![];

        if settings.base_image.as_path() == Path::new("/") {
            let dirs = global_settings
                .exposed_host_items
                .as_ref()
                .unwrap_or_else(|| &*DEFAULT_HOST_MOUNTS);
            for item in dirs {
                let item = format!("/{}", item);
                let shared_item = minion::SharedItem {
                    id: None,
                    src: item.clone().into(),
                    dest: item.into(),
                    kind: minion::SharedItemKind::Readonly,
                    flags: Vec::new(),
                };
                shared_items.push(shared_item)
            }
        } else {
            let toolchain_dir = &settings.base_image;
            let mut opt_items = fs::read_dir(toolchain_dir).await.with_context(|| {
                format!(
                    "failed to list toolchain image directory ({})",
                    toolchain_dir.display()
                )
            })?;
            while let Some(item) = opt_items.next_entry().await? {
                let name = item.file_name();
                let shared_item = minion::SharedItem {
                    id: None,
                    src: toolchain_dir.join(&name),
                    dest: PathBuf::from(&name),
                    kind: minion::SharedItemKind::Readonly,
                    flags: Vec::new(),
                };
                shared_items.push(shared_item)
            }
        }

        for item in &settings.expose {
            let kind = match item.mode {
                SharedDirectoryMode::ReadOnly => minion::SharedItemKind::Readonly,
                SharedDirectoryMode::ReadWrite => minion::SharedItemKind::Full,
            };
            let host_path = path_resolver.resolve(&item.host_path)?;
            if item.create {
                tokio::fs::create_dir_all(&host_path).await?;
            }
            // TODO: is this best way?
            {
                let current_mode = tokio::fs::metadata(&host_path).await?.permissions().mode();
                if let SharedDirectoryMode::ReadWrite = item.mode {
                    // copies access for owner to access for group and others
                    let our_access = (current_mode >> 6) & 0b111;
                    let mode = (current_mode >> 9 << 9) | (our_access * ((1 << 6) + (1 << 3) + 1));
                    let perms = PermissionsExt::from_mode(mode);
                    tracing::debug!(
                        "changing permissions for {} from {:o} to {:o}",
                        host_path.display(),
                        current_mode,
                        mode
                    );
                    tokio::fs::set_permissions(&host_path, perms).await?;
                }
            }
            let shared_item = minion::SharedItem {
                id: None,
                src: host_path.clone(),
                dest: item.sandbox_path.clone(),
                kind,
                flags: Vec::new(),
            };
            shared_items.push(shared_item);
        }

        tokio::fs::create_dir_all(&sandbox_data_dir)
            .await
            .context("failed to create sandbox data directory")?;

        for item in &shared_items {
            validate_shared_item(item).await;
        }

        let cpu_time_limit = Duration::from_millis(settings.limits.time);
        let real_time_limit = Duration::from_millis(settings.limits.time * 3);
        let chroot_dir = sandbox_data_dir.join("root");
        tokio::fs::create_dir(&chroot_dir)
            .await
            .with_context(|| format!("failed to create chroot dir {}", chroot_dir.display()))?;
        // TODO adjust integer types
        let sandbox_options = minion::SandboxOptions {
            max_alive_process_count: settings
                .limits
                .process_count
                .unwrap_or_else(|| default_process_limit(global_settings))
                as _,
            memory_limit: settings.limits.memory,
            shared_items,
            isolation_root: chroot_dir,
            cpu_time_limit,
            real_time_limit,
        };
        tracing::debug!(options = ?sandbox_options, "Creating minion sandbox");
        let sandbox = backend
            .new_sandbox(sandbox_options)
            .context("failed to create minion sandbox")?;

        let raw_debug_data = sandbox
            .debug_info()
            .context("failed to get sandbox debugging information")?;
        let debug_data = AttachRequest {
            raw: raw_debug_data,
            request_id,
            sandbox_name: settings.name.clone(),
        };
        global_settings
            .suspender
            .suspend(debug_data)
            .await
            .context("failed to wait for debugger attach")?;

        Ok(Sandbox { sandbox })
    }

    /// Makes sure that inner sandbox will not be dropped
    pub fn leak(&self) {
        tracing::info!("preventing cleanup for the sandbox");
        let t = self.sandbox.clone();
        std::mem::forget(t);
    }
}

static DEFAULT_HOST_MOUNTS: once_cell::sync::Lazy<Vec<String>> = once_cell::sync::Lazy::new(|| {
    vec![
        "usr".to_string(),
        "bin".to_string(),
        "lib".to_string(),
        "lib64".to_string(),
    ]
});

async fn validate_shared_item(item: &SharedItem) {
    if let Err(e) = do_validate_shared_item(item).await {
        tracing::warn!(
            "Exposed path {} seems to be unusable: {:#}",
            item.src.display(),
            e
        );
    }
}

async fn do_validate_shared_item(item: &SharedItem) -> anyhow::Result<()> {
    match tokio::fs::metadata(&item.src).await {
        Ok(meta) => {
            let perm = meta.permissions().mode();
            // since sandbox is executed as a `nobody`-like user,
            // we are interested in three lowest bits
            let access_for_others = perm & 0o7;
            let desired_access = match item.kind {
                SharedItemKind::Full => 0b111,
                SharedItemKind::Readonly => 0b101,
            };
            if access_for_others & desired_access != desired_access {
                anyhow::bail!(
                    "Expected `others` access {}, but actually it is {}",
                    desired_access,
                    access_for_others
                );
            }
            Ok(())
        }

        Err(err) => Err(err).context("path it not accessible to invoker"),
    }
}
