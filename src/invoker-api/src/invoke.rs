//! # Invocation API
//! Requests invoker to execute commands, specified in
//! `steps` field in request.
//! ## Endpoints
//! `POST /exec`
//! ## Execution order
//! Each step has assigned `stage`.
//! Steps with equal stage will be executed in the same time.
//! Such steps can share pipes. Sharing pipes between steps from
//! different stages results in error. For each stage,
//! steps creating new IPC stuff are executed first and then commands are run.
//! Step will not be executed until all steps with less `stage`
//! will be finished.
//! ## Data
//! `InvokeRequest` can specify input data items, that can be further used
//! as stdin for executed commands (input data item can be used several times).
//! ## DataRequest
//! `InvokeRequest` can specify output data requests, which will be populated
//! from some files, created by `CreateFile` action.
//! ## Extensions
//! You can specify extensions at different levels of the InvokeRequest obkect.
//! This extensions must be consumed and stripped by the shim.

use serde::{ser::Error as _, Deserialize, Serialize};
use std::{fmt, path::PathBuf};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct InvokeRequest {
    /// Set of commands that must be executed
    pub steps: Vec<Step>,
    /// Binary data used for executing commands
    pub inputs: Vec<Input>,
    /// Binary data produced by executing commands
    pub outputs: Vec<OutputRequest>,
    /// Request identifier.
    /// Will be returned as-is in response.
    pub id: uuid::Uuid,
    #[serde(default)]
    pub ext: Extensions,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct InvokeResponse {
    /// Request identifier as specified in request.
    pub id: uuid::Uuid,
    /// Outputs for all OutputRequest (the same order as in request).
    pub outputs: Vec<Output>,
    /// Results of all actions
    pub actions: Vec<ActionResult>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct OutputRequest {
    /// Request name (will be preserved)
    pub name: String,
    /// Specifies what exactly should be exported
    pub target: OutputRequestTarget,
    #[serde(default)]
    pub ext: Extensions,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub enum OutputRequestTarget {
    /// File id that should be exported
    File(FileId),
    /// Export local file by path
    Path(PathBuf),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// File id that must be assigned to this input
    pub file_id: FileId,
    /// Data source
    pub source: InputSource,
    #[serde(default)]
    pub ext: Extensions,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub enum InputSource {
    /// Data available as file on FS
    LocalFile { path: PathBuf },
    /// Data provided inline as string
    InlineString { data: String },
    /// Data provided inline as a base64-encoded string
    InlineBase64 { data: String },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Output {
    /// Output name (as specified in OutputRequest)
    pub name: String,
    /// Data itself
    pub data: OutputData,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub enum OutputData {
    /// Base64-encoded data
    InlineBase64(String),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub stage: u32,
    pub action: Action,
    #[serde(default)]
    pub ext: Extensions,
}

/// Newtype identifier of file-like object, e.g. real file or pipe.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct FileId(pub String);

// this makes formatting identifiers more ergonomic.
impl fmt::Display for FileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// Name of sandbox, created earlier in this request
    pub sandbox_name: String,
    pub argv: Vec<String>,
    pub env: Vec<EnvironmentVariable>,
    pub cwd: String,
    pub stdio: Stdio,
    #[serde(default)]
    pub ext: Extensions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: EnvVarValue,
    #[serde(default)]
    pub ext: Extensions,
}

/// Allowed access to shared directory
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub enum SharedDirectoryMode {
    /// R-X
    ReadOnly,
    /// RWX
    ReadWrite,
}

/// Piece of filesystem that should be exposed to sandbox.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SharedDir {
    /// Absolute path inside the selected root.
    pub host_path: PathBuf,
    /// Absolute path inside the sandbox.
    pub sandbox_path: PathBuf,
    /// Access mode
    pub mode: SharedDirectoryMode,
    /// If true and hostPath does not exist, it will be created.
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub ext: Extensions,
}

/// Value of the environment variable
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub enum EnvVarValue {
    /// Use this string as a value
    Plain(String),
    /// Pass stringified handle (aka fd) of this file as a value.
    /// For example, "45".
    File(FileId),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Stdio {
    pub stdin: FileId,
    pub stdout: FileId,
    pub stderr: FileId,
    #[serde(default)]
    pub ext: Extensions,
}

/// Describer limits that should be applied to a sandbox.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Memory limit in bytes
    pub memory: u64,
    /// Time limit in milliseconds
    pub time: u64,
    /// Process count limit.
    #[serde(default)]
    pub process_count: Option<u64>,
    /// Working dir size limit in bytes
    #[serde(default)]
    pub work_dir_size: Option<u64>,
    #[serde(default)]
    pub ext: Extensions,
}

/// Sandbox settings
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SandboxSettings {
    /// Limits enforced for processes in the sandbox.
    pub limits: Limits,
    /// Sandbox name.
    pub name: String,
    /// Directory to use as a sandbox filesystem base.
    /// Special case is "/". In this case invoker will mount not all rootfs
    /// contents, but only items mentioned in `--expose-rootfs-item` flag,
    /// or built-in default set of items.
    pub base_image: PathBuf,
    /// Additional paths to mount into sandbox.
    pub expose: Vec<SharedDir>,
    /// Path to the working directory.
    /// Initially it will be empty, and it will be readable and writable
    /// by all sandboxed processes.
    pub work_dir: PathBuf,
    #[serde(default)]
    pub ext: Extensions,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub enum ActionResult {
    CreatePipe,
    CreateFile,
    OpenFile,
    OpenNullFile,
    ExecuteCommand(CommandResult),
    CreateSandbox,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct CommandResult {
    /// If this field is set, command failed to start.
    /// Other fields will have unspecified values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_error: Option<uuid::Uuid>,
    /// Process exit code
    pub exit_code: i64,
    /// CPU time usage in nanoseconds (but precision will be likely coarser).
    pub cpu_time: Option<u64>,
    /// Memory usage in bytes (but precision will be likely coarser).
    pub memory: Option<u64>,
}

/// Single action of execution plan.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub enum Action {
    /// Specifies that a pipe must be allocated.
    /// Use CreatePipe instead if you want to export
    /// written data as an output instead of sending
    /// it to another program
    CreatePipe {
        /// Will be associated with pipe's read half
        read: FileId,
        /// Will be associated with pipe's write half
        write: FileId,
    },
    /// Specifies that a file must be created.
    /// At least one of `readable` and `writeable`
    /// must be set to true.
    CreateFile {
        /// Will be associated with the file
        id: FileId,
        /// Open file in read mode
        readable: bool,
        /// Open file in write mode
        writeable: bool,
    },
    /// Associates file on local fs with a FileId
    OpenFile {
        /// Path to the file
        path: PathBuf,
        /// Id to associate with file
        id: FileId,
    },
    /// Associates file id with read-only empty file, e.g. `/dev/null`.
    OpenNullFile { id: FileId },
    /// Specifies that command should be executed
    ExecuteCommand(Command),
    /// Specifies that sandbox should be created
    CreateSandbox(SandboxSettings),
}

/// Arbitrary key-value data (i.e. json Object), consumed by the shim.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(transparent)]
pub struct Extensions(pub serde_json::Map<String, serde_json::Value>);

impl Extensions {
    pub fn make<T: Serialize>(value: T) -> Result<Self, serde_json::Error> {
        let value = serde_json::to_value(value)?;
        if let serde_json::Value::Object(m) = value {
            return Ok(Extensions(m));
        }
        Err(serde_json::Error::custom(
            "Extensions can only hold JSON object",
        ))
    }
}
