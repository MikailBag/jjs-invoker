mod cli_args;
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
use cli_args::IdRange;
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
    /// User and group id range
    /// When invoker runs as root, it will assign identifiers from this range
    /// to the sandboxes.
    ///
    /// Must be specified as `LOW:HIGH`, where LOW < HIGH, and range [LOW, HIGH)
    /// will be used.
    #[clap(long)]
    sandbox_id_range: Option<IdRange>,
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
        override_id_range: args.sandbox_id_range.as_ref().map(|r| (r.low, r.high)),
    };
    let handler = Handler::new(handler_cfg, sandbox_cfg)
        .await
        .context("failed to initialize handler")?;
    let shim = ShimClient::new(args.shim.as_deref()).context("failed to initialize shim client")?;
    let server = server::Server::new(handler, shim);
    server.serve(args.listen_address.clone()).await
}
