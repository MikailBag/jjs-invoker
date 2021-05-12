use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Context;
use invoker_api::invoke::{PathPrefix, PrefixedPath};

pub struct PathResolver {
    volumes: HashMap<String, PathBuf>,
}

impl PathResolver {
    pub fn new() -> Self {
        PathResolver {
            volumes: HashMap::new(),
        }
    }

    pub fn add_volume(&mut self, name: &str, root: &Path) {
        self.volumes.insert(name.to_string(), root.to_path_buf());
    }

    fn resolve_prefix(&self, prefix: &PathPrefix) -> anyhow::Result<PathBuf> {
        match prefix {
            PathPrefix::Host => Ok("/".into()),
            PathPrefix::Volume(name) => self
                .volumes
                .get(name)
                .cloned()
                .with_context(|| format!("volume {} not found", name)),
            // we could ban it during validation, but this works too
            PathPrefix::Extension(_) => {
                anyhow::bail!("Extension sharedDirSource must be resolved by the shim")
            }
        }
    }

    pub fn resolve(&self, src: &PrefixedPath) -> anyhow::Result<PathBuf> {
        if !src.path.is_relative() {
            anyhow::bail!("prefixed path must be relative")
        }
        self.resolve_prefix(&src.prefix).map(|p| p.join(&src.path))
    }
}
