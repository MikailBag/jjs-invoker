use anyhow::Context;
use clap::Clap;
use rand::Rng;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clap)]
struct CliArgs {
    #[clap(long)]
    image: String,
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
    let invoker_image = &args.image;
    xshell::cmd!("docker inspect --format=OK {invoker_image}").run()?;

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
        let (base_dir_path, image_tag) =
            prepare_base_image(&name, &item.path(), Path::new("/tmp"))?;
        println!("Starting container");
        let container_name = randomize(&format!("jjs-invoker-test-suite-{}", name));
        let work_dir_path = logs_dir.join(&name);
        let test_files = item
            .path()
            .join("files")
            .canonicalize()
            .context("failed to canonicalize ./files dir")?;
        std::fs::create_dir_all(&work_dir_path)?;
        xshell::cmd!(
            "docker create 
            --publish-all 
            --cgroupns host
            --name {container_name}
            --mount type=bind,ro=true,src={base_dir_path},dst=/base
            --mount type=bind,src={work_dir_path},dst=/var/judges
            --mount type=bind,src={test_files},dst=/test
            --memory 2g
            --env RUST_BACKTRACE=1
            --env RUST_LOG=info,invoker=trace
            --privileged
            {invoker_image}"
        )
        .run()?;
        xshell::cmd!("docker start {container_name}").run().ok();
        println!("Waiting for container readiness");
        {
            let mut ready = false;
            for _ in 0..10 {
                let description = docker_describe(&container_name)?;
                let health_status = description
                    .pointer("/0/State/Health/Status")
                    .and_then(|val| val.as_str());
                println!("Health status: {:?}", health_status);
                if health_status == Some("healthy") {
                    ready = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2500));
            }
            if !ready {
                anyhow::bail!("readiness wait timed out");
            }
        }
        let port = {
            let description = docker_describe(&container_name)?;
            let port_jsonpath = "/0/NetworkSettings/Ports/8000~1tcp/0/HostPort";
            let port = description.pointer(port_jsonpath).with_context(|| {
                format!("{} path missing in container description", port_jsonpath)
            })?;
            port.as_str()
                .context("port is not string")?
                .parse()
                .context("invalid HostPort value")?
        };
        if args.wait_before_test {
            wait();
        }
        let res = run_test(&name, &item.path(), port, &image_tag);
        if !args.retain_containers {
            xshell::cmd!("docker kill {container_name}").run().ok();
        }
        xshell::cmd!("docker logs {container_name}").run().ok();
        if !args.retain_containers {
            xshell::cmd!("docker rm {container_name}").run()?;
        }
        if let Some(err) = res.err() {
            return Err(err).with_context(|| format!("test {} failed", name));
        }
    }

    Ok(())
}

fn prepare_base_image(
    test_name: &str,
    test_case: &Path,
    tmp_dir: &Path,
) -> anyhow::Result<(PathBuf, String)> {
    println!("Building base image");
    let image_tag = format!("jjs-invoker-tests-base-image-{}", test_name);
    xshell::cmd!("docker build -t {image_tag} {test_case}").run()?;
    let image_hash = {
        let description = docker_describe(&image_tag)?;
        let path = "/0/Id";
        let image = description
            .pointer(path)
            .and_then(|val| val.as_str())
            .and_then(|val| val.strip_prefix("sha256:"))
            .context("failed to obtain image hash")?;
        image.to_string()
    };
    let export_path = tmp_dir.join(image_hash);
    let tar_path = export_path.join("img.tar");
    let unpacked_path = export_path.join("img");
    if unpacked_path.exists() {
        return Ok((unpacked_path, image_tag));
    }
    let container_name = randomize(&image_tag);
    xshell::cmd!("docker create --name={container_name} {image_tag}").run()?;
    std::fs::create_dir_all(&unpacked_path)?;
    xshell::cmd!("docker export {container_name} --output {tar_path}").run()?;
    {
        let _d = xshell::pushd(&unpacked_path)?;
        xshell::cmd!("tar xf ../img.tar").run()?;
    }
    xshell::cmd!("docker rm {container_name}").run()?;
    Ok((unpacked_path, image_tag))
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

fn run_test(test_name: &str, test_case: &Path, port: u16, image_tag: &str) -> anyhow::Result<()> {
    let client = reqwest::blocking::ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(100))
        .build()?;
    let addr = format!("http://localhost:{}/exec", port);
    let request_body =
        std::fs::read(test_case.join("request.yaml")).context("failed to read request.yaml")?;
    let mut request_body: serde_json::Value =
        serde_yaml::from_slice(&request_body).context("invalid request")?;
    let img_envs = {
        let description = docker_describe(image_tag)?;
        let env_items = description
            .pointer("/0/Config/Env")
            .context("Env missing in image config")?
            .as_array()
            .context("env is not array")?
            .clone();

        env_items
            .into_iter()
            .map(|env_item| {
                let env_item = env_item
                    .as_str()
                    .expect("docker image config should specify env as list of strings");
                let mut it = env_item.splitn(2, '=');
                let name = it.next().expect("env var name missing");
                let value = it.next().expect("env var value missing");
                serde_json::json!({
                    "name": name,
                    "value": {
                        "plain": value,
                    }
                })
            })
            .collect::<Vec<_>>()
    };
    {
        let request_body = request_body.as_object_mut().unwrap();
        request_body.insert(
            "id".to_string(),
            serde_json::Value::String(Uuid::new_v4().to_hyphenated().to_string()),
        );
        request_body
            .get_mut("steps")
            .expect("steps missing in request")
            .as_array_mut()
            .expect("steps is not array")
            .iter_mut()
            .for_each(|step| {
                let exec = step
                    .get_mut("action")
                    .expect("action missing")
                    .as_object_mut()
                    .expect("action is not object")
                    .get_mut("executeCommand");
                if let Some(exec) = exec {
                    let envs = exec
                        .as_object_mut()
                        .expect("executeCommand is not object")
                        .get_mut("env")
                        .expect("env missing")
                        .as_array_mut()
                        .expect("env is not array");
                    envs.extend(img_envs.clone());
                }
            });
    }
    let request_body_json = request_body.clone();
    let request_body = serde_json::to_string_pretty(&request_body)?;
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
        let response: serde_json::Value = response
            .json()
            .context("failed to deserialize response body")?;
        let export_path = randomize(&format!("/tmp/jjs-invoker-test-{}-outputs", test_name));
        let export_path: PathBuf = export_path.into();
        std::fs::create_dir(&export_path)?;
        export_response(&request_body_json, &response, &export_path)?;
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

fn export_response(
    req: &serde_json::Value,
    res: &serde_json::Value,
    path: &Path,
) -> anyhow::Result<()> {
    let request_outputs = req.pointer("/outputs").unwrap().as_array().unwrap();
    let response_outputs = res.pointer("/outputs").unwrap().as_array().unwrap();
    assert_eq!(request_outputs.len(), response_outputs.len());
    for (req_out, res_out) in request_outputs.iter().zip(response_outputs.iter()) {
        let output_name = req_out.pointer("/fileId").unwrap().as_str().unwrap();
        let output_value = res_out.pointer("/inlineBase64").unwrap().as_str().unwrap();
        let output_value = base64::decode(output_value).context("invalid base64")?;
        std::fs::write(path.join(output_name), output_value)?;
    }
    Ok(())
}

fn docker_describe(name: &str) -> anyhow::Result<serde_json::Value> {
    let description = xshell::cmd!("docker inspect {name}")
        .read()
        .context("failed to describe container")?;
    let description = description.trim();
    serde_json::from_str(description).context("failed to parse inspect output")
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
