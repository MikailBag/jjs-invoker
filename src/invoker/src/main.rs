mod config;
mod executor;
mod graph_interp;
mod handler;
mod init;
mod print_invoke_request;
mod server;
mod shim;

use anyhow::Context;
use clap::Clap;
use executor::SandboxGlobalSettings;
use handler::{Handler, HandlerConfig};
use shim::ShimClient;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Clap, Debug)]
struct CliArgs {
    /// Skip optional system checkes.
    #[clap(long)]
    skip_checks: bool,
    /// Directory to store intermediate files.
    #[clap(long)]
    work_dir: PathBuf,
    /// Listen address.
    /// Example for TCP: tcp://0.0.0.0:8000
    /// Example for unix sockets: unix:/run/jjs-invoker.sock
    #[clap(long)]
    listen_address: server::ListenAddress,
    /// Shim address.
    /// For example, `https://127.0.0.1:8001`
    #[clap(long)]
    shim: Option<String>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args: CliArgs = Clap::parse();
    tracing::debug!(args = ?args);
    init::init()?;
    real_main(args)
}

#[tokio::main]
async fn real_main(args: CliArgs) -> anyhow::Result<()> {
    let handler_cfg = HandlerConfig {
        work_dir: args.work_dir,
    };
    let sandbox_cfg = SandboxGlobalSettings {
        // TODO: add CLI arg
        exposed_host_items: None,
        skip_system_checks: args.skip_checks,
    };
    let handler = Handler::new(handler_cfg, sandbox_cfg)
        .await
        .context("failed to initialize handler")?;
    let shim = ShimClient::new(args.shim.as_deref()).context("failed to initialize shim client")?;
    let server = server::Server::new(handler, shim);
    server.serve(args.listen_address.clone()).await
}
