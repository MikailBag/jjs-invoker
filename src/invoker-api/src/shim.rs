//! Standard shim API

use crate::invoke::InputSource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const EXTRA_FILES_DIR_NAME: &str = "EXTRA_FILES";

/// See RequestExtensions::extra_files
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ExtraFile {
    /// File contents
    pub contents: InputSource,
    /// Make file executable
    #[serde(default)]
    pub executable: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct RequestExtensions {
    /// HashMap from path to file contents.
    /// This files will be written to directory that can be
    /// referred in mounts as value of the EXTRA_FILES_DIR_NAME constant.
    #[serde(default)]
    pub extra_files: HashMap<String, ExtraFile>,
    /// Values to substitute into `commands`.
    /// Keys will be automatically prefixed with `Request.`.
    #[serde(default)]
    pub substitutions: HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SandboxSettingsExtensions {
    /// Image that contains toolchain files.
    pub image: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SharedDirExtensionSource {
    /// Name of the special source
    pub name: String,
}
