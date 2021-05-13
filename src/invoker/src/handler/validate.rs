use invoker_api::invoke::{Action, Command, InvokeRequest, SandboxSettings, Step, VolumeSettings};

fn request_has_extensions(req: &InvokeRequest) -> bool {
    let InvokeRequest {
        steps,
        inputs,
        outputs,
        id: _,
        ext,
    } = req;

    if !ext.0.is_empty() {
        return true;
    }
    for step in steps {
        if step_has_extensions(step) {
            return true;
        }
    }
    for inp in inputs {
        if !inp.ext.0.is_empty() {
            return true;
        }
    }

    for out in outputs {
        if !out.ext.0.is_empty() {
            return true;
        }
    }

    false
}

fn step_has_extensions(step: &Step) -> bool {
    let Step {
        stage: _,
        action,
        ext,
    } = step;
    if !ext.0.is_empty() {
        return true;
    }
    match action {
        Action::CreateSandbox(sb) => sandbox_has_extensions(sb),
        Action::CreatePipe { read: _, write: _ } => false,
        Action::CreateFile {
            id: _,
            readable: _,
            writeable: _,
        } => false,
        Action::OpenFile { path: _, id: _ } => false,
        Action::OpenNullFile { id: _ } => false,
        Action::ExecuteCommand(cmd) => command_has_extensions(cmd),
        Action::CreateVolume(vol) => volume_has_extensions(vol),
    }
}

fn command_has_extensions(cmd: &Command) -> bool {
    let Command {
        sandbox_name: _,
        argv: _,
        env,
        cwd: _,
        stdio,
        ext,
    } = cmd;
    if !ext.0.is_empty() {
        return true;
    }
    if !stdio.ext.0.is_empty() {
        return true;
    }
    for e in env {
        if !e.ext.0.is_empty() {
            return true;
        }
    }
    false
}

fn sandbox_has_extensions(sb: &SandboxSettings) -> bool {
    let SandboxSettings {
        limits,
        name: _,
        base_image: _,
        expose,
        ext,
    } = sb;

    if !ext.0.is_empty() {
        return true;
    }
    if !limits.ext.0.is_empty() {
        return true;
    }
    for exp in expose {
        if !exp.ext.0.is_empty() {
            return true;
        }
    }
    false
}

fn volume_has_extensions(vol: &VolumeSettings) -> bool {
    !vol.ext.0.is_empty()
}

pub(super) fn validate_request(req: &InvokeRequest) -> anyhow::Result<()> {
    if request_has_extensions(req) {
        anyhow::bail!("Request contains non-empty extensions")
    }
    Ok(())
}
