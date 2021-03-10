use std::path::Path;

use anyhow::Context as _;

/// A JJS invoker
pub struct Env {
    invoker: Container,
    shim: Container,
    registry: Container,
    _network: Network,
    _toolchains: Volume,
}

impl Env {
    pub fn new(
        name: &str,
        work_dir: &Path,
        invoker_image: &str,
        shim_image: &str,
    ) -> anyhow::Result<Self> {
        let network_name = format!("{}-net", name);
        let network = Network::create(&network_name)?;

        let volume_name = format!("{}-toolchains", name);
        let volume = Volume::create(&volume_name)?;

        let invoker_mounts = vec![
            format!("type=bind,src={},dst=/var/judges", work_dir.display()),
            format!("type=volume,src={},dst=/toolchains,readonly", volume_name),
        ];
        let invoker = Container::create(
            &format!("{}-invoker", name),
            ContainerSettings {
                image: invoker_image,
                mounts: &invoker_mounts,
                network: &network_name,
                network_name: "invoker",
                privileged: true,
                args: &[
                    "--work-dir".to_string(),
                    "/var/judges".to_string(),
                    "--listen-address".to_string(),
                    "tcp://0.0.0.0:8000".to_string(),
                    "--shim=http://shim:8001".to_string(),
                ],
                health: None,
            },
        )?;
        let shim = Container::create(
            &format!("{}-shim", name),
            ContainerSettings {
                image: shim_image,
                mounts: &[format!(
                    "type=volume,src={},dst=/pull-toolchains-here",
                    volume_name
                )],
                network: &network_name,
                network_name: "shim",
                privileged: false,
                args: &[
                    "--port=8001".to_string(),
                    "--allow-remote".to_string(),
                    "--disable-pull-tls".to_string(),
                    "--exchange-dir=/pull-toolchains-here".to_string(),
                    "--invoker-exchange-dir=/toolchains".to_string(),
                ],
                health: None,
            },
        )?;

        let registry = Container::create(
            &format!("{}-registry", name),
            ContainerSettings {
                image: "registry:2",
                mounts: &[],
                network: &network_name,
                network_name: "registry",
                privileged: false,
                args: &[],
                health: Some("echo"),
            },
        )?;

        Ok(Env {
            invoker,
            shim,
            registry,
            _network: network,
            _toolchains: volume,
        })
    }

    pub fn start(&self) -> anyhow::Result<()> {
        self.invoker.start()?;
        self.shim.start()?;
        self.registry.start()?;
        Ok(())
    }

    pub fn kill(&self) {
        self.invoker.kill();
        self.shim.kill();
        self.registry.kill();
    }

    pub fn logs(&self) -> anyhow::Result<()> {
        self.invoker.logs()?;
        self.shim.logs()?;
        self.registry.logs()?;
        Ok(())
    }

    pub fn health(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![
            self.invoker.health()?,
            self.shim.health()?,
            self.registry.health()?,
        ])
    }

    pub fn invoker_port(&self) -> anyhow::Result<u16> {
        self.invoker.resolve_port(8000)
    }

    pub fn registry_port(&self) -> anyhow::Result<u16> {
        self.registry.resolve_port(5000)
    }
}

struct Container {
    name: String,
}

struct ContainerSettings<'a> {
    image: &'a str,
    mounts: &'a [String],
    network: &'a str,
    network_name: &'a str,
    privileged: bool,
    args: &'a [String],
    health: Option<&'a str>,
}

impl Container {
    fn create(name: &str, settings: ContainerSettings<'_>) -> anyhow::Result<Self> {
        let network = settings.network;
        let net_name = settings.network_name;
        let mut cmd = xshell::cmd!(
            "docker create 
            --publish-all 
            --name {name}
            --network {network}
            --hostname {net_name}
            --network-alias {net_name}
            --memory 4g
            --env RUST_BACKTRACE=1
            --env RUST_LOG=info,invoker=trace,shim=trace,puller=trace"
        );
        for mount in settings.mounts {
            cmd = cmd.arg("--mount").arg(mount);
        }
        if settings.privileged {
            cmd = cmd.arg("--privileged");
        }
        if let Some(h) = settings.health {
            cmd = cmd.arg("--health-cmd").arg(h);
            cmd = cmd.arg("--health-timeout").arg("10s");
            cmd = cmd.arg("--health-interval").arg("6s");
        }
        cmd = cmd.arg(settings.image);
        cmd = cmd.args(settings.args);
        cmd.run()?;
        Ok(Container {
            name: name.to_string(),
        })
    }

    fn start(&self) -> anyhow::Result<()> {
        let name = &self.name;
        xshell::cmd!("docker start {name}").run()?;
        Ok(())
    }

    fn logs(&self) -> anyhow::Result<()> {
        let name = &self.name;
        xshell::cmd!("docker logs {name}").run()?;
        Ok(())
    }

    fn kill(&self) {
        let name = &self.name;
        xshell::cmd!("docker kill {name}").run().ok();
    }

    fn describe(&self) -> anyhow::Result<serde_json::Value> {
        let name = &self.name;
        let description = xshell::cmd!("docker inspect {name}")
            .read()
            .context("failed to describe container")?;
        let description = description.trim();
        serde_json::from_str(description).context("failed to parse inspect output")
    }

    fn health(&self) -> anyhow::Result<String> {
        let description = self.describe()?;
        let health_status = description
            .pointer("/0/State/Health/Status")
            .and_then(|s| s.as_str())
            .unwrap_or("<missing>");
        Ok(health_status.to_string())
    }

    fn resolve_port(&self, port: u16) -> anyhow::Result<u16> {
        let description = self.describe()?;
        let port_jsonpath = format!("/0/NetworkSettings/Ports/{}~1tcp/0/HostPort", port);
        let port = description
            .pointer(&port_jsonpath)
            .with_context(|| format!("{} path missing in container description", port_jsonpath))?;
        port.as_str()
            .context("port is not string")?
            .parse()
            .context("invalid HostPort value")
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let name = &self.name;
        xshell::cmd!("docker rm --force {name}").run().ok();
    }
}

struct Network {
    name: String,
}

impl Network {
    fn create(name: &str) -> anyhow::Result<Self> {
        xshell::cmd!("docker network create {name}").run()?;
        Ok(Network {
            name: name.to_string(),
        })
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        let name = &self.name;
        xshell::cmd!("docker network rm {name}").run().ok();
    }
}

struct Volume {
    name: String,
}

impl Volume {
    fn create(name: &str) -> anyhow::Result<Self> {
        xshell::cmd!("docker volume create {name}").run()?;
        Ok(Volume {
            name: name.to_string(),
        })
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        let name = &self.name;
        xshell::cmd!("docker volume rm {name}").run().ok();
    }
}
