use anyhow::Context;
use std::path::{Path, PathBuf};

fn generate_compose_config(
    work_dir: &Path,
    invoker_image: &str,
    shim_image: &str,
) -> serde_json::Value {
    serde_json::json!({
        "services": {
            "invoker": {
                "image": invoker_image,
                "volumes": [
                    {
                        "type": "bind",
                        "source": format!("{}/judges", work_dir.display()),
                        "target": "/var/judges"
                    },
                    {
                        "type": "volume",
                        "source": "toolchains",
                        "target": "/toolchains",
                        "read_only": true,
                    }
                ],
                "privileged": true,
                "command":[
                    "--work-dir",
                    "/var/judges",
                    "--listen-address",
                    "tcp://0.0.0.0:8000",
                    "--shim=http://shim:8001",
                ],
                "environment": {
                    "RUST_BACKTRACE": "1",
                    "RUST_LOG": "info,invoker=trace",
                },
                "ports": ["8000"]
            },
            "shim": {
                "image": shim_image,
                "volumes": [
                    {
                        "type": "volume",
                        "source": "toolchains",
                        "target": "/pull-toolchains-here"
                    }
                ],
                "command": [
                    "--port=8001",
                    "--allow-remote",
                    "--disable-pull-tls",
                    "--exchange-dir=/pull-toolchains-here",
                    "--invoker-exchange-dir=/toolchains",
                ],
                "environment": {
                    "RUST_BACKTRACE": "1",
                    "RUST_LOG": "info,puller=trace,shim=trace",
                }
            },
            "registry": {
                "image": "docker.io/library/registry:2",
                "ports": ["5000"]
            }
        },
        "volumes": {
            "toolchains": {}
        }
    })
}

/// A JJS invoker
pub struct Env {
    compose_dir: PathBuf,
    name: String,
}

impl Env {
    pub fn new(
        name: &str,
        work_dir: &Path,
        invoker_image: &str,
        shim_image: &str,
    ) -> anyhow::Result<Self> {
        let compose_dir = work_dir.join("compose");
        std::fs::create_dir_all(&compose_dir)?;
        std::fs::create_dir_all(work_dir.join("judges"))?;

        let config = generate_compose_config(work_dir, invoker_image, shim_image);
        let config = serde_json::to_string_pretty(&config)?;
        std::fs::write(compose_dir.join("docker-compose.yaml"), config)?;

        Ok(Env {
            compose_dir,
            name: name.to_string(),
        })
    }

    fn in_dir<T, F: FnOnce() -> anyhow::Result<T>>(&self, func: F) -> anyhow::Result<T> {
        let _p = xshell::pushd(&self.compose_dir)?;
        func()
    }

    pub fn start(&self) -> anyhow::Result<()> {
        self.in_dir(|| {
            let name = &self.name;
            xshell::cmd!("docker-compose --project-name {name} up --detach").run()?;
            Ok(())
        })
    }

    pub fn kill(&self) {
        let r = self.in_dir(|| {
            let name = &self.name;
            xshell::cmd!("docker-compose --project-name {name} kill").run()?;
            Ok(())
        });
        if let Err(e) = r {
            eprintln!("kill error: {:#}", e);
        }
    }

    pub fn logs(&self) -> anyhow::Result<()> {
        self.in_dir(|| {
            let name = &self.name;
            xshell::cmd!("docker-compose --project-name {name} logs").run()?;
            Ok(())
        })
    }

    pub fn health(&self) -> anyhow::Result<Vec<String>> {
        // TODO this is hacky
        std::thread::sleep(std::time::Duration::from_secs(1));
        Ok(vec![])
    }

    fn resolve_port(&self, svc: &str, port: u16) -> anyhow::Result<u16> {
        self.in_dir(|| {
            let name = &self.name;
            let port = port.to_string();
            let out =
                xshell::cmd!("docker-compose --project-name {name} port {svc} {port}").read()?;
            let out = out.trim();
            let out = out.strip_prefix("0.0.0.0:").context("unexpected binding")?;
            let p = out.parse()?;
            Ok(p)
        })
    }

    pub fn invoker_port(&self) -> anyhow::Result<u16> {
        self.resolve_port("invoker", 8000)
    }

    pub fn registry_port(&self) -> anyhow::Result<u16> {
        self.resolve_port("registry", 5000)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        self.kill();
    }
}
