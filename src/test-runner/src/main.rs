mod env;

use anyhow::Context;
use clap::Clap;
use env::Env;
use invoker_api::invoke::{Action, Extensions, InvokeRequest, InvokeResponse, OutputData};
use rand::Rng;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clap)]
struct CliArgs {
    #[clap(long)]
    invoker_image: String,
    #[clap(long)]
    shim_image: String,
    #[clap(long)]
    logs: PathBuf,
    #[clap(long)]
    test_cases_path: Option<PathBuf>,
    #[clap(long)]
    retain_containers: bool,
    #[clap(long)]
    wait_before_test: bool,
}

fn main() -> anyhow::Result<()> {
    let args: CliArgs = Clap::parse();
    let test_cases_path = args
        .test_cases_path
        .clone()
        .unwrap_or_else(|| "./test-data".into());

    std::fs::read_dir(&args.logs).context("logs dir does not exist")?;
    let invoker_image = &args.invoker_image;
    xshell::cmd!("docker inspect --format=OK {invoker_image}").run()?;
    let shim_image = &args.shim_image;
    xshell::cmd!("docker inspect --format=OK {shim_image}").run()?;

    if !test_cases_path.exists() {
        anyhow::bail!("Path {} does not exist", test_cases_path.display());
    }
    let items = std::fs::read_dir(test_cases_path)?.collect::<Result<Vec<_>, _>>()?;

    let logs_dir = args
        .logs
        .canonicalize()
        .context("failed to canonicalize logs path")?;

    for item in items {
        if !item.file_type()?.is_dir() {
            anyhow::bail!("{} is not directory", item.file_name().to_string_lossy());
        }
        let name = item
            .file_name()
            .to_str()
            .context("test case name is not utf-8")?
            .to_string();
        println!("--- Running test {} ---", name);
        let image_tag = prepare_base_image(&name, &item.path())?;
        println!("Starting environment");
        let work_dir_path = logs_dir.join(&name);

        std::fs::create_dir_all(&work_dir_path)?;
        let env_name = randomize(&format!("jjs-invoker-test-suite-{}", name));
        let e = Env::new(&env_name, &work_dir_path, invoker_image, shim_image)?;

        e.start()?;
        println!("Waiting for container readiness");
        {
            let mut ready = false;
            for _ in 0..10 {
                let health = e.health()?;
                println!("Health status: {:?}", health);
                if health.iter().all(|h| *h == "healthy" || *h == "<missing>") {
                    ready = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(3000));
            }
            if !ready {
                e.logs()?;
                anyhow::bail!("readiness wait timed out");
            }
        }
        println!("Pushing test image");
        {
            let dest = format!("localhost:{}/{}", e.registry_port()?, name);
            xshell::cmd!(
                "skopeo copy --dest-tls-verify=false 
                docker-daemon:{image_tag}:latest 
                docker://{dest}:latest"
            )
            .run()?;
        }
        if args.wait_before_test {
            wait();
        }
        let port = e.invoker_port()?;
        let res = run_test(&name, &item.path(), port);
        if !args.retain_containers {
            e.kill();
        }
        e.logs()?;
        if args.retain_containers {
            println!("Leaking docker resources as requested");
            std::mem::forget(e);
        }
        if let Some(err) = res.err() {
            return Err(err).with_context(|| format!("test {} failed", name));
        }
    }

    Ok(())
}

fn prepare_base_image(test_name: &str, test_case: &Path) -> anyhow::Result<String> {
    println!("Building base image");
    let image_tag = format!("jjs-invoker-tests-base-image-{}", test_name);
    xshell::cmd!("docker build -t {image_tag} {test_case}").run()?;
    Ok(image_tag)
}

fn wait() {
    let file_name = format!("start-test-{}", rand_suf());
    println!("Waiting for approval");
    println!("Run following command to start test:");
    println!("\ttouch {}", file_name);
    loop {
        if std::fs::metadata(&file_name).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn run_test(test_name: &str, test_case: &Path, port: u16) -> anyhow::Result<()> {
    let client = reqwest::blocking::ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(100))
        .build()?;
    let addr = format!("http://localhost:{}/exec", port);
    let invoke_request =
        std::fs::read(test_case.join("request.yaml")).context("failed to read request.yaml")?;
    let mut invoke_request: invoker_api::invoke::InvokeRequest =
        serde_yaml::from_slice(&invoke_request).context("invalid request")?;

    let expected_id = Uuid::new_v5(&Uuid::nil(), test_name.as_bytes());
    if invoke_request.id != expected_id {
        anyhow::bail!("request ID must be {}", expected_id.to_hyphenated())
    }
    let sandbox_settings_extensions = invoker_api::shim::SandboxSettingsExtensions {
        image: format!("registry:5000/{}", test_name),
    };
    for step in &mut invoke_request.steps {
        let action = &mut step.action;
        if let Action::CreateSandbox(sb) = action {
            sb.ext = Extensions::make(&sandbox_settings_extensions)?;
        }
    }
    let request_body = serde_json::to_string_pretty(&invoke_request)?;
    let response = client
        .post(addr.as_str())
        .body(request_body)
        .send()
        .context("request failed")?;
    if response.status().is_client_error() {
        let response = response.text()?;
        anyhow::bail!("request failed:\n{}", response);
    } else if response.status().is_server_error() {
        anyhow::bail!("invocation fault")
    } else {
        let response: InvokeResponse = response
            .json()
            .context("failed to deserialize response body")?;
        let export_path = randomize(&format!("/tmp/jjs-invoker-test-{}-outputs", test_name));
        let export_path: PathBuf = export_path.into();
        std::fs::create_dir(&export_path)?;
        export_response(&invoke_request, &response, &export_path)?;
        {
            let test_case = test_case.canonicalize()?;
            let _d = xshell::pushd(&export_path)?;
            xshell::cmd!("python3 {test_case}/validate.py")
                .run()
                .context("validation script failed")?;
        }
    }
    Ok(())
}

fn export_response(req: &InvokeRequest, res: &InvokeResponse, path: &Path) -> anyhow::Result<()> {
    let request_outputs = req.outputs.as_slice();
    let response_outputs = res.outputs.as_slice();
    assert_eq!(request_outputs.len(), response_outputs.len());
    for (req_out, res_out) in request_outputs.iter().zip(response_outputs.iter()) {
        let output_name = req_out.name.clone();
        println!("Exporting output {}", output_name);
        let output_value = match &res_out.data {
            OutputData::InlineBase64(v) => v,
            OutputData::None => anyhow::bail!("missing output"),
        };
        let output_value = base64::decode(output_value).context("invalid base64")?;
        std::fs::write(path.join(output_name), output_value)?;
    }
    Ok(())
}

fn rand_suf() -> String {
    let mut base = String::new();
    let mut rng = rand::thread_rng();
    for _ in 0..5 {
        base.push(rng.sample(rand::distributions::Alphanumeric) as char);
    }
    base
}

fn randomize(base: &str) -> String {
    format!("{}-{}", base, rand_suf())
}
