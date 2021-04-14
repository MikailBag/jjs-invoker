mod interp;
mod toolchain;
mod transform;

use clap::Clap;
use futures::future::TryFutureExt;
use invoker_api::{invoke::InvokeRequest, shims::ShimResponse};
use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    sync::Arc,
};
use toolchain::ToolchainPuller;
use tracing_subscriber::EnvFilter;
use warp::Filter;

struct ServerState {
    toolchain_puller: Arc<ToolchainPuller>,
    invoker_exchange_dir: PathBuf,
    local_exchange_dir: PathBuf,
}

#[tracing::instrument(skip(state, req), fields(request_id = %req.id.to_hyphenated()))]
async fn route_on_request_inner(
    state: Arc<ServerState>,
    mut req: InvokeRequest,
) -> anyhow::Result<ShimResponse> {
    transform::transform_request(
        &mut req,
        state.toolchain_puller.clone(),
        &state.local_exchange_dir,
        &state.invoker_exchange_dir,
    )
    .await?;

    Ok(ShimResponse::Result(req))
}

async fn route_on_request(
    state: Arc<ServerState>,
    req: serde_json::Value,
) -> anyhow::Result<ShimResponse> {
    let req = match serde_json::from_value(req) {
        Ok(r) => r,
        Err(err) => {
            return Ok(ShimResponse::Error(serde_json::Value::String(format!(
                "invalid request body: parse error: {:#}",
                err
            ))));
        }
    };
    route_on_request_inner(state, req).await
}

async fn route_ready() -> Result<&'static str, Infallible> {
    Ok("OK")
}

#[derive(Debug)]
struct AnyhowRejection(anyhow::Error);

impl warp::reject::Reject for AnyhowRejection {}

#[derive(Clap)]
struct Args {
    /// Listen port
    #[clap(long)]
    port: u16,
    /// Allow external connections
    #[clap(long)]
    allow_remote: bool,
    /// Use insecure HTTP protocol when pulling images
    #[clap(long)]
    disable_pull_tls: bool,
    /// Directory for exchanging data with invoker
    #[clap(long)]
    exchange_dir: PathBuf,
    /// Path to `--exchange-dir` from invoker's PoV
    #[clap(long)]
    invoker_exchange_dir: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args: Args = Clap::parse();

    let toolchain_puller =
        ToolchainPuller::new(args.disable_pull_tls, &args.exchange_dir.join("toolchains")).await?;
    let toolchain_puller = Arc::new(toolchain_puller);
    let state = ServerState {
        toolchain_puller,
        invoker_exchange_dir: args.invoker_exchange_dir.clone(),
        local_exchange_dir: args.exchange_dir.clone(),
    };
    let state = Arc::new(state);

    let r_on_req = warp::path("on-request")
        .and(warp::filters::body::json())
        .and_then(move |req| {
            route_on_request(state.clone(), req)
                .map_ok(|resp| {
                    hyper::Response::builder()
                        .status(resp.http_status())
                        .body(serde_json::to_vec(&resp).expect("failed to serialize response"))
                })
                .map_err(|err| warp::reject::custom(AnyhowRejection(err)))
        });

    let r_ready = warp::path("ready").and_then(route_ready);

    #[cfg(debug_assertions)]
    let r_on_req = r_on_req.boxed();
    #[cfg(debug_assertions)]
    let r_ready = r_ready.boxed();

    let routes = r_on_req.or(r_ready);

    let srv = warp::serve(routes.with(warp::filters::trace::request()));

    let listen_addr = if args.allow_remote {
        Ipv4Addr::new(0, 0, 0, 0)
    } else {
        Ipv4Addr::new(127, 0, 0, 1)
    };

    let addr = SocketAddr::V4(SocketAddrV4::new(listen_addr, args.port));
    srv.run(addr).await;

    Ok(())
}
