use crate::api::{SandboxSettings, SharedDirectoryMode};
use anyhow::Context as _;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::fs;
use tracing::{debug, error};

pub struct Sandbox {
    sandbox: Box<dyn minion::erased::Sandbox>,
    umount: Option<PathBuf>,
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
        work_dir: &Path,
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

        tokio::fs::create_dir_all(&work_dir)
            .await
            .context("failed to create working directory")?;
        let umount_path;
        #[cfg(target_os = "linux")]
        {
            let quota = settings.limits.work_dir_size;
            let quota = minion::linux::ext::Quota::bytes(quota);
            minion::linux::ext::make_tmpfs(&work_dir.join("data"), quota)
                .context("failed to set size limit on shared directory")?;
            umount_path = Some(work_dir.join("data"));
        }
        #[cfg(not(target_os = "linux"))]
        {
            umount_path = None;
        }
        shared_dirs.push(minion::SharedDir {
            src: work_dir.join("data"),
            dest: settings.work_dir.clone(),
            kind: minion::SharedDirKind::Full,
        });
        let cpu_time_limit = Duration::from_millis(settings.limits.time);
        let real_time_limit = Duration::from_millis(settings.limits.time * 3);
        let chroot_dir = work_dir.join("root");
        tokio::fs::create_dir(&chroot_dir)
            .await
            .with_context(|| format!("failed to create chroot dir {}", chroot_dir.display()))?;
        // TODO adjust integer types
        let sandbox_options = minion::SandboxOptions {
            max_alive_process_count: settings.limits.process_count as _,
            memory_limit: settings.limits.memory,
            exposed_paths: shared_dirs,
            isolation_root: work_dir.join("root"),
            cpu_time_limit,
            real_time_limit,
        };
        let sandbox = backend
            .new_sandbox(sandbox_options)
            .context("failed to create minion dominion")?;
        Ok(Sandbox {
            sandbox,
            umount: umount_path,
        })
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Some(p) = self.umount.take() {
            if let Err(err) = nix::mount::umount2(&p, nix::mount::MntFlags::MNT_DETACH) {
                error!("Leaking tmpfs at {}: umount2 failed: {}", p.display(), err)
            } else {
                debug!("Successfully destroyed tmpfs at {}", p.display())
            }
        } else {
            panic!("TODO, REMOVE: winda??")
        }
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
