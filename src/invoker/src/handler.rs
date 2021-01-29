use crate::{
    api::{InvokeRequest, InvokeResponse, Output},
    executor::{Executor, SandboxGlobalSettings},
    graph_interp::Interpreter,
    print_invoke_request::PrintWrapper,
};
use anyhow::Context as _;
use minion::{erased::Backend, linux::Settings};
use std::path::PathBuf;

pub struct HandlerConfig {
    pub work_dir: PathBuf,
}

pub struct Handler {
    cfg: HandlerConfig,
    sandbox_global_settings: SandboxGlobalSettings,
    minion_backend: Box<dyn Backend>,
}

fn check_system(settings: &Settings) -> anyhow::Result<()> {
    let mut errs = minion::CheckResult::new();
    minion::linux::check::check(settings, &mut errs);
    tracing::info!("System check outcome: {}", errs);
    if errs.has_errors() {
        anyhow::bail!("invoker is not able to serve invocation requests: {}", errs);
    }
    Ok(())
}

fn setup_minion(skip_checks: bool) -> anyhow::Result<Box<dyn Backend>> {
    let mut settings = Settings::new();
    settings.cgroup_prefix = "/jjs".into();
    if !skip_checks {
        check_system(&settings).context("system configuration problem detected")?;
    }
    let backend = minion::LinuxBackend::new(settings)?;
    Ok(Box::new(backend))
}

impl Handler {
    pub async fn new(
        config: HandlerConfig,
        sandbox_global_settings: SandboxGlobalSettings,
    ) -> anyhow::Result<Self> {
        let backend = setup_minion(sandbox_global_settings.skip_system_checks)
            .context("failed to initialize minion backend")?;

        Ok(Handler {
            cfg: config,
            sandbox_global_settings,
            minion_backend: backend,
        })
    }

    fn print_request(&self, req: &InvokeRequest) {
        let wrapper = PrintWrapper(req);
        let msg = wrapper.print();
        tracing::info!(request = msg.as_str(), "processing InvokeRequest");
    }

    #[tracing::instrument(skip(self, req))]
    pub async fn handle_invoke_request(
        &self,
        req: &InvokeRequest,
    ) -> anyhow::Result<InvokeResponse> {
        self.print_request(req);
        let per_request_work_dir = self.cfg.work_dir.join(req.id.to_hyphenated().to_string());
        let mut interp = Interpreter::new(req);
        let mut exec = Executor::new(
            &per_request_work_dir,
            &*self.minion_backend,
            &self.sandbox_global_settings,
        );

        for input in &req.inputs {
            exec.add_input(input)
                .with_context(|| format!("Failed to add input file {}", input.file_id))?;
        }

        let mut response = InvokeResponse {
            id: req.id,
            outputs: Vec::new(),
            actions: Vec::new(),
        };

        let mut started_steps = Vec::new();
        let mut poll_input = Vec::new();
        loop {
            tracing::trace!(interpreter_state = ?interp, input = ?poll_input, "polling interpreter");
            let resp = interp.poll(&poll_input);
            poll_input.clear();
            if resp.is_empty() {
                break;
            }
            tracing::trace!(response = ?resp, "interpreter responsed");
            for step_id in &resp {
                let step_id = *step_id;
                if started_steps.contains(&step_id) {
                    continue;
                }
                tracing::info!(step_id = step_id, "Starting step");
                started_steps.push(step_id);
                let action_result = exec
                    .run_action(&req.steps[step_id].action)
                    .await
                    .context("Step failed")?;
                tracing::info!(step_id = step_id, "Finished step");
                poll_input.push(step_id);
                response.actions.push(action_result);
            }
        }
        if !interp.is_completed() {
            anyhow::bail!("Internal error: interpreter stuck: no new steps were requested");
        }
        tracing::info!("Collecting outputs");

        for (pos, output_req) in req.outputs.iter().enumerate() {
            let data = exec
                .export(&output_req.file_id)
                .await
                .with_context(|| format!("failed to export #{}", pos))?;
            tracing::debug!(output_id = pos, byte_count = data.len());
            let data = base64::encode(&data);

            response.outputs.push(Output::InlineBase64(data));
        }
        Ok(response)
    }
}