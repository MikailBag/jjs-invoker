use crate::api::{SandboxSettings, SharedDirectoryMode};
use anyhow::Context as _;
use minion::{SharedDir, SharedDirKind};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::fs;
use tracing::{debug, error};

/// Owns `tmpfs`, mounted into sandbox.
struct Tmpfs {
    path: PathBuf,
}

impl Tmpfs {
    #[cfg(target_os = "linux")]
    fn new(settings: &SandboxSettings, path: &Path) -> anyhow::Result<Self> {
        let quota = settings.limits.work_dir_size;
        let quota = minion::linux::ext::Quota::bytes(quota);
        minion::linux::ext::make_tmpfs(path, quota)
            .context("failed to set size limit on shared directory")?;
        Ok(Tmpfs {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for Tmpfs {
    fn drop(&mut self) {
        if let Err(err) = nix::mount::umount2(&self.path, nix::mount::MntFlags::MNT_DETACH) {
            error!(
                "Leaking tmpfs at {}: umount2 failed: {}",
                self.path.display(),
                err
            )
        } else {
            debug!("Successfully destroyed tmpfs at {}", self.path.display())
        }
    }
}

pub struct Sandbox {
    sandbox: Box<dyn minion::erased::Sandbox>,
    // RAII owner
    _tmpfs: Tmpfs,
}

pub struct SandboxGlobalSettings {
    pub exposed_host_items: Option<Vec<String>>,
    pub skip_system_checks: bool,
}

impl Sandbox {
    pub fn raw_sandbox(&self) -> Box<dyn minion::erased::Sandbox> {
        self.sandbox.clone()
    }

    pub async fn create(
        sandbox_data_dir: &Path,
        backend: &dyn minion::erased::Backend,
        settings: &SandboxSettings,
        global_settings: &SandboxGlobalSettings,
    ) -> anyhow::Result<Self> {
        let mut shared_dirs = vec![];

        if settings.base_image.as_path() == Path::new("/") {
            let dirs = global_settings
                .exposed_host_items
                .as_ref()
                .unwrap_or_else(|| &*DEFAULT_HOST_MOUNTS);
            for item in dirs {
                let item = format!("/{}", item);
                let shared_dir = minion::SharedDir {
                    src: item.clone().into(),
                    dest: item.into(),
                    kind: minion::SharedDirKind::Readonly,
                };
                shared_dirs.push(shared_dir)
            }
        } else {
            let toolchain_dir = &settings.base_image;
            let mut opt_items = fs::read_dir(toolchain_dir)
                .await
                .context("failed to list toolchains sysroot")?;
            while let Some(item) = opt_items.next_entry().await? {
                let name = item.file_name();
                let shared_dir = minion::SharedDir {
                    src: toolchain_dir.join(&name),
                    dest: PathBuf::from(&name),
                    kind: minion::SharedDirKind::Readonly,
                };
                shared_dirs.push(shared_dir)
            }
        }

        for item in &settings.expose {
            let kind = match item.mode {
                SharedDirectoryMode::ReadOnly => minion::SharedDirKind::Readonly,
                SharedDirectoryMode::ReadWrite => minion::SharedDirKind::Full,
            };
            let shared_dir = minion::SharedDir {
                src: item.host_path.clone(),
                dest: item.sandbox_path.clone(),
                kind,
            };
            shared_dirs.push(shared_dir);
        }

        tokio::fs::create_dir_all(&sandbox_data_dir)
            .await
            .context("failed to create sandbox data directory")?;

        let sandbox_work_dir = sandbox_data_dir.join("data");

        let t = Tmpfs::new(settings, &sandbox_work_dir)
            .context("failed to allocate sandbox working directory")?;

        shared_dirs.push(minion::SharedDir {
            src: sandbox_work_dir,
            dest: settings.work_dir.clone(),
            kind: minion::SharedDirKind::Full,
        });

        for item in &shared_dirs {
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
            max_alive_process_count: settings.limits.process_count as _,
            memory_limit: settings.limits.memory,
            exposed_paths: shared_dirs,
            isolation_root: chroot_dir,
            cpu_time_limit,
            real_time_limit,
        };
        let sandbox = backend
            .new_sandbox(sandbox_options)
            .context("failed to create minion dominion")?;
        Ok(Sandbox { sandbox, _tmpfs: t })
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

async fn validate_shared_item(item: &SharedDir) {
    if let Err(e) = do_validate_shared_item(item).await {
        tracing::warn!(
            "Exposed path {} seems to be unusable: {:#}",
            item.src.display(),
            e
        );
    }
}

async fn do_validate_shared_item(item: &SharedDir) -> anyhow::Result<()> {
    match tokio::fs::metadata(&item.src).await {
        Ok(meta) => {
            let perm = meta.permissions().mode();
            // since sandbox is executed as a `nobody`-like user,
            // we are interested in three lowest bits
            let access_for_others = perm & 0o7;
            let desired_access = match item.kind {
                SharedDirKind::Full => 0b111,
                SharedDirKind::Readonly => 0b101,
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

        Err(err) => return Err(err).context("path it not accessible to invoker"),
    }
}
