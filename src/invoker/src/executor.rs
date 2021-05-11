mod file;
mod sandbox;

pub use sandbox::SandboxGlobalSettings;

use self::{file::File, sandbox::Sandbox};
use anyhow::Context;
use invoker_api::invoke::{
    Action, ActionResult, CommandResult, EnvVarValue, FileId, Input, InputSource,
};
use minion::{
    erased::ChildProcessOptions, InputSpecification, OutputSpecification, StdioSpecification,
};
use std::{
    collections::{
        hash_map::{Entry, VacantEntry},
        HashMap,
    },
    path::Path,
};

/// Actually executes steps from the InvokeRequest and handles Inputs&Outputs.
pub struct Executor<'a> {
    files: HashMap<FileId, File>,
    /// Map from sandbox name to sandbox object
    sandboxes: HashMap<String, Sandbox>,
    work_dir: &'a Path,
    minion: &'a dyn minion::erased::Backend,
    sandbox_global_settings: &'a SandboxGlobalSettings,
}

impl<'a> Executor<'a> {
    pub fn new(
        work_dir: &'a Path,
        minion: &'a dyn minion::erased::Backend,
        sandbox_global_settings: &'a SandboxGlobalSettings,
    ) -> Self {
        Executor {
            work_dir,
            files: HashMap::new(),
            sandboxes: HashMap::new(),
            minion,
            sandbox_global_settings,
        }
    }

    pub fn add_input(&mut self, input: &Input) -> anyhow::Result<()> {
        let slot = self.prepare_entry(&input.file_id)?;
        let file = match &input.source {
            InputSource::InlineString { data } => {
                File::from_buffer(data.as_bytes(), "jjs-invoker")?
            }
            InputSource::InlineBase64 { data } => {
                let data = base64::decode(&data).context("invalid base64")?;
                File::from_buffer(&data, "jjs-invoker")?
            }
            InputSource::LocalFile { path } => File::open_read(&path)?,
        };
        slot.insert(file);
        Ok(())
    }

    pub async fn export(&mut self, id: &FileId) -> anyhow::Result<Vec<u8>> {
        let file = self.files.get(id).context("unknown file id")?;
        file.read_all().await
    }

    /// Prepates a slot for later `File` insertion.
    /// Validates that file_id is unused.
    fn prepare_entry(&mut self, id: &FileId) -> anyhow::Result<VacantEntry<FileId, File>> {
        match self.files.entry(id.clone()) {
            Entry::Occupied(_occ) => {
                anyhow::bail!("File with id {} already exists", id);
            }
            Entry::Vacant(v) => Ok(v),
        }
    }

    pub async fn run_action(&mut self, action: &Action) -> anyhow::Result<ActionResult> {
        match action {
            Action::CreateFile {
                id,
                readable,
                writeable,
            } => {
                let file_path = self.work_dir.join(&format!("files/{}", id));
                let slot = self.prepare_entry(id)?;
                let open_func = match (*readable, *writeable) {
                    (false, false) => anyhow::bail!("Neither readable nor writeable flags are set"),
                    (true, false) => |p| File::open_read(p),
                    (false, true) => |p| File::open_write(p),
                    (true, true) => |p| File::open_read_write(p),
                };
                let file = open_func(&file_path).context("failed to create file")?;
                slot.insert(file);
                Ok(ActionResult::CreateFile)
            }
            Action::OpenFile { path, id } => {
                let slot = self.prepare_entry(id)?;
                let file = File::open_read(path)
                    .with_context(|| format!("failed to open {}", path.display()))?;
                slot.insert(file);
                Ok(ActionResult::OpenFile)
            }
            Action::OpenNullFile { id } => {
                let slot = self.prepare_entry(id)?;
                let file = File::open_null().context("failed to open null file")?;
                slot.insert(file);
                Ok(ActionResult::OpenNullFile)
            }
            Action::CreatePipe { read, write } => {
                // unfortunately, we create pipe before checking IDs are unused.
                // however it's not big issue, since pipe creation usually doesn't fail,
                // as opposed to e.g. opening files.
                let (reader, writer) = File::pipe().context("failed to create pipe")?;
                let slot_reader = self.prepare_entry(read)?;
                slot_reader.insert(reader);
                let slot_writer = self.prepare_entry(write)?;
                slot_writer.insert(writer);
                Ok(ActionResult::CreatePipe)
            }
            Action::CreateSandbox(sandbox_settings) => {
                if self.sandboxes.contains_key(&sandbox_settings.name) {
                    anyhow::bail!("Sandbox named {} already created", sandbox_settings.name);
                }
                let sandbox = Sandbox::create(
                    &self.work_dir.join("sandboxes").join(&sandbox_settings.name),
                    &*self.minion,
                    sandbox_settings,
                    &self.sandbox_global_settings,
                )
                .await
                .context("failed to create sandbox")?;
                self.sandboxes
                    .insert(sandbox_settings.name.clone(), sandbox);
                Ok(ActionResult::CreateSandbox)
            }
            Action::ExecuteCommand(command) => {
                let sandbox = match self.sandboxes.get(&command.sandbox_name) {
                    Some(s) => s,
                    None => anyhow::bail!("Unknown sandbox {}", command.sandbox_name),
                };
                let sandbox = sandbox.raw_sandbox();
                if command.argv.is_empty() {
                    anyhow::bail!("argv must be non-empty");
                }

                let stdin = self
                    .files
                    .get(&command.stdio.stdin)
                    .context("stdin references unknown file")?;
                stdin
                    .check_readable()
                    .context("stdin is not readable file")?;

                let stdout = self
                    .files
                    .get(&command.stdio.stdout)
                    .context("stdout references unknown file")?;
                stdout
                    .check_writable()
                    .context("stdout is not readable file")?;

                let stderr = self
                    .files
                    .get(&command.stdio.stderr)
                    .context("stderr references unknown file")?;
                stderr
                    .check_writable()
                    .context("stderr is not readable file")?;

                let stdin = stdin.try_clone_inherit()?;
                let stdout = stdout.try_clone_inherit()?;
                let stderr = stderr.try_clone_inherit()?;

                let stdio = StdioSpecification {
                    stdin: InputSpecification::handle(stdin.into_raw()),
                    stdout: OutputSpecification::handle(stdout.into_raw()),
                    stderr: OutputSpecification::handle(stderr.into_raw()),
                };
                let mut opts = ChildProcessOptions {
                    path: (&command.argv[0]).into(),
                    arguments: command
                        .argv
                        .get(1..)
                        .unwrap()
                        .iter()
                        .map(|arg| arg.into())
                        .collect(),
                    environment: Vec::new(),
                    sandbox: sandbox.clone(),
                    pwd: command.cwd.as_str().into(),
                    stdio,
                };
                let mut inherited_files = Vec::new();
                for env in &command.env {
                    let value = match &env.value {
                        EnvVarValue::File(id) => {
                            let file = self.files.get(id).context("env references unknown file")?;
                            let clone = file
                                .try_clone_inherit()
                                .context("failed to create inheritable file copy")?;
                            let s = clone.as_raw().to_string();
                            inherited_files.push(clone);
                            s
                        }
                        EnvVarValue::Plain(plain) => plain.clone(),
                    };
                    let kv = format!("{}={}", env.name, value);
                    opts.environment.push(kv.into());
                }
                tracing::debug!(options = ?opts, "Creating child process");
                let mut child_process = match self.minion.spawn(opts) {
                    Ok(ch) => ch,
                    Err(err) => {
                        let spawn_error_id = uuid::Uuid::new_v4();
                        tracing::info!(error_id = %spawn_error_id.to_hyphenated(), error=?err, "Failed to spawn command");
                        return Ok(ActionResult::ExecuteCommand(CommandResult {
                            spawn_error: Some(spawn_error_id),
                            exit_code: i64::max_value(),
                            cpu_time: None,
                            memory: None,
                        }));
                    }
                };
                let exit_code = child_process
                    .wait_for_exit()
                    .context("failed to start child process exit watch")?
                    .await
                    .context("wait error")?
                    .0;
                // TODO we should report resource usage as part of sandbox, not command
                let resource_usage = sandbox
                    .resource_usage()
                    .context("failed to capture resource usage")?;
                Ok(ActionResult::ExecuteCommand(CommandResult {
                    spawn_error: None,
                    exit_code,
                    cpu_time: resource_usage.time,
                    memory: resource_usage.memory,
                }))
            }
        }
    }
}
