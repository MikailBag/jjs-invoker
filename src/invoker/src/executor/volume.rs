use anyhow::Context;
use invoker_api::invoke::VolumeSettings;
use std::path::{Path, PathBuf};

/// Owns a volume.
pub struct Volume {
    path: PathBuf,
    has_tmpfs: bool,
}

impl Volume {
    #[cfg(target_os = "linux")]
    pub async fn create(settings: &VolumeSettings, path: &Path) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(path)
            .await
            .context("failed to create volume directory")?;
        if let Some(quota) = settings.limit {
            let quota = minion::linux::ext::Quota::bytes(quota);
            minion::linux::ext::make_tmpfs(path, quota)
                .context("failed to set size limit on shared directory")?;
        }
        Ok(Volume {
            path: path.to_path_buf(),
            has_tmpfs: settings.limit.is_some(),
        })
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        if !self.has_tmpfs {
            return;
        }
        if let Err(err) = nix::mount::umount2(&self.path, nix::mount::MntFlags::MNT_DETACH) {
            tracing::error!(
                "Leaking tmpfs at {}: umount2 failed: {}",
                self.path.display(),
                err
            )
        } else {
            tracing::debug!("Successfully destroyed tmpfs at {}", self.path.display())
        }
    }
}
