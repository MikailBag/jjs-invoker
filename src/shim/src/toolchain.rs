//! This module is responsible for pulling toolchain images

use anyhow::Context as _;
use dkregistry::v2::manifest::{Manifest, RuntimeConfig};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone)]
pub struct PulledToolchain {
    /// Last path portion
    pub path: String,
    image_config: ImageConfig,
}

impl PulledToolchain {
    /// Returns environment variables defined by the image
    pub fn get_env(&self) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        for (k, v) in self.image_config.environment.clone() {
            env.insert(k, v);
        }
        env
    }
}

/// Contains some data, extracted from image manifest
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ImageConfig {
    pub environment: Vec<(String, String)>,
}

impl ImageConfig {
    fn parse_env_item(item: &str) -> Option<(String, String)> {
        let mut parts = item.splitn(2, '=');
        let key = parts.next()?;
        let value = parts.next()?;
        Some((key.to_string(), value.to_string()))
    }

    fn from_run_config(rc: RuntimeConfig) -> anyhow::Result<Self> {
        let environment = rc
            .env
            .unwrap_or_default()
            .into_iter()
            .map(|item| ImageConfig::parse_env_item(&item))
            .map(|item| item.context("environment string does not look like key=value"))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self { environment })
    }
}

/// Responsible for fetching toolchains
pub struct ToolchainPuller {
    image_puller: puller::Puller,
    toolchains_dir: PathBuf,
    // TODO: per-registry options
    disable_tls: bool,
    /// Cache for already pulled toolchains.
    cache: Mutex<HashMap<String, PulledToolchain>>,
}

impl ToolchainPuller {
    pub async fn new(disable_tls: bool, tooclhains_dir: &Path) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(tooclhains_dir)
            .await
            .with_context(|| {
                format!(
                    "failed to create directory for storing pulled toolchains ({})",
                    tooclhains_dir.display()
                )
            })?;
        let image_puller = puller::Puller::new().await;
        Ok(ToolchainPuller {
            image_puller,
            toolchains_dir: tooclhains_dir.to_path_buf(),
            disable_tls,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Actually downloads and unpacks toolchain to specified dir.
    #[tracing::instrument(skip(self, toolchain_image, target_dir))]
    async fn extract_toolchain(
        &self,
        toolchain_image: &str,
        target_dir: &Path,
    ) -> anyhow::Result<ImageConfig> {
        tracing::info!(target_dir=%target_dir.display(), "downloading image");

        // touched if toolchain was downloaded successfully
        let ok_file = target_dir.join(".pull-success");

        let already_pulled = tokio::fs::metadata(&ok_file).await.is_ok();
        if already_pulled {
            tracing::info!(
                "image is already available in local filesystem, skipping layers download"
            )
        } else {
            if tokio::fs::metadata(target_dir).await.is_ok() {
                tracing::info!("cleaning up previous failed pull attempt");
                tokio::fs::remove_dir_all(target_dir)
                    .await
                    .with_context(|| format!("failed to remove {}", target_dir.display()))?;
            }
            tokio::fs::create_dir(target_dir)
                .await
                .context("failed to create target dir")?;
        }

        let settings = {
            let tls = if self.disable_tls {
                puller::Tls::Disable
            } else {
                puller::Tls::Enable
            };

            puller::PullSettings {
                tls,
                skip_layers: already_pulled,
            }
        };
        tracing::debug!(settings = ?settings, "generated pull settings");
        let image_manifest = self
            .image_puller
            .pull(
                toolchain_image,
                target_dir,
                settings,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .with_context(|| format!("failed to pull toolchain image {}", toolchain_image))?;
        let image_manifest = match image_manifest {
            Manifest::S2(im_v2) => im_v2,
            _ => anyhow::bail!("Unsupported manifest: only schema2 is supported"),
        };

        let runtime_config = image_manifest
            .config_blob
            .runtime_config
            .context("image manifest does not have RunConfig")?;

        let image_config = ImageConfig::from_run_config(runtime_config)
            .context("failed to process config blob")?;
        tokio::fs::write(ok_file, "OK")
            .await
            .context("failed to touch .pull-success file")?;
        tracing::info!("toolchain has been pulled successfully");
        Ok(image_config)
    }

    #[tracing::instrument(skip(self))]
    pub async fn resolve(&self, toolchain_image: &str) -> anyhow::Result<PulledToolchain> {
        {
            // TODO: do not pull one image concurrently, instead one task should pull
            // ond other should wait.
            let cache = self.cache.lock().unwrap();
            if let Some(info) = cache.get(toolchain_image) {
                return Ok(info.clone());
            }
        }
        let dirname = base64::encode(toolchain_image);
        let toolchain_dir = self.toolchains_dir.join(&dirname);

        let image_config = self
            .extract_toolchain(toolchain_image, &toolchain_dir)
            .await
            .context("toolchain download error")?;

        let pt = PulledToolchain {
            path: dirname,
            image_config,
        };
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(toolchain_image.to_string(), pt.clone());
        }
        Ok(pt)
    }
}
