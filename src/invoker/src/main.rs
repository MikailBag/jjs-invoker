mod cli_args;
mod config;
mod executor;
mod graph_interp;
mod handler;
mod init;
mod interactive_debug;
mod print_invoke_request;
mod server;
mod shim;

use anyhow::Context;
use clap::Clap;
use cli_args::IdRange;
use executor::SandboxGlobalSettings;
use handler::{Handler, HandlerConfig};
use shim::ShimClient;
use std::{path::PathBuf, sync::Arc};
use tracing_subscriber::{filter::EnvFilter, fmt::format::FmtSpan};

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
    /// Do not cleanup sandboxes.
    /// This flag completely disables sandbox destruction logic, making
    /// debugging more simple.
    ///
    /// CAUTION: setting this flags means that invoker will leak memory,
    /// file descriptors and other system resources on each request.
    #[clap(long)]
    debug_leak_sandboxes: bool,
    /// Enables file-based interactive debugging mode.
    ///
    /// This flag takes a path to the existing directory as an argument.
    /// Invoker will print path that must be touched for sandbox to resume
    #[clap(long, conflicts_with = "interactive-debug-url")]
    interactive_debug_path: Option<PathBuf>,
    /// Enables HTTP-based interactive debugging mode.
    ///
    /// This flag receives a url as an argument. Invoker will send POST requests
    /// to this url and will resume when successful response is returned.
    #[clap(long, conflicts_with = "interactive-debug-path")]
    interactive_debug_url: Option<String>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_span_events(FmtSpan::CLOSE)
        .init();
    let args: CliArgs = Clap::parse();
    tracing::debug!(args = ?args);
    if args.debug_leak_sandboxes {
        tracing::warn!("dangerous --debug-leak-sandboxes flag was enabled");
    }
    init::init()?;
    real_main(args)
}

#[tokio::main]
async fn real_main(args: CliArgs) -> anyhow::Result<()> {
    let handler_cfg = HandlerConfig {
        work_dir: args.work_dir.clone(),
    };

    let interactive_debug_suspender = interactive_debug::Suspender::new(&args);

    let sandbox_cfg = SandboxGlobalSettings {
        // TODO: add CLI arg
        exposed_host_items: None,
        skip_system_checks: args.skip_checks,
        override_id_range: args.sandbox_id_range.as_ref().map(|r| (r.low, r.high)),
        leak: args.debug_leak_sandboxes,
        // TODO: revisit when rootless mode is added
        allow_fallback_pid_limit: false,
        suspender: Arc::new(interactive_debug_suspender),
    };
    let handler = Handler::new(handler_cfg, sandbox_cfg)
        .await
        .context("failed to initialize handler")?;
    let shim = ShimClient::new(args.shim.as_deref()).context("failed to initialize shim client")?;
    let server = server::Server::new(handler, shim);
    server.serve(args.listen_address.clone()).await
}
