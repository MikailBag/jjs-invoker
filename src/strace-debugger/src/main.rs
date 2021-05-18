use anyhow::Context;
use clap::Clap;
use invoker_api::debug::AttachRequest;
use serde::Deserialize;
use std::{ffi::OsString, path::PathBuf, sync::Arc};
use tracing::Instrument;
use tracing_subscriber::EnvFilter;
use warp::Filter;

#[derive(Clap)]
struct Args {
    /// Bind port
    #[clap(long, default_value = "8000")]
    listen_port: u16,
    /// Directory that will contain traces
    #[clap(long, default_value = "/var/jjs/debug/strace")]
    dump_dir: PathBuf,
    /// Custom strace binary
    #[clap(long, default_value = "strace")]
    strace: OsString,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawData {
    zygote_pid: u32,
}

async fn handler(data: serde_json::Value, args: Arc<Args>) -> anyhow::Result<()> {
    let data: AttachRequest =
        serde_json::from_value(data).context("failed to parse attach request")?;
    let raw_data: RawData = serde_json::from_value(data.raw).context("failed to parse raw data")?;
    let dir = args
        .dump_dir
        .join(data.request_id.to_hyphenated().to_string());
    tokio::fs::create_dir_all(&dir)
        .await
        .context("failed to create directory for storing log")?;
    let path = dir.join(&data.sandbox_name);
    let sp = tracing::info_span!("collecting strace",
        request_id = %data.request_id.to_hyphenated(),
        sandbox_name = data.sandbox_name.as_str());
    // now we carefully synchronize with backgound task:
    // while we do not want to wait until strace is complete,
    // we want to minimize the change that some event will be used.
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn(
        async move {
            tracing::info!("starting strace");
            let mut cmd = tokio::process::Command::new(&args.strace);
            cmd.arg("-f");
            cmd.arg("-o").arg(path);
            cmd.arg("-p").arg(raw_data.zygote_pid.to_string());
            let mut ch = match cmd.spawn() {
                Ok(c) => c,
                Err(err) => {
                    tracing::warn!("failed to spawn strace: {:#}", err);
                    return;
                }
            };
            // here strace was already launched.
            // give it some time to initialize
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // and now let's **hope** that has attached
            // to the sandbox
            tracing::info!("resuming sandbox");
            tx.send(()).ok();

            let st = match ch.wait().await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!("failed to wait for strace: {:#}", err);
                    return;
                }
            };
            if !st.success() {
                tracing::warn!("strace failed: {:?}", st);
            }
        }
        .instrument(sp),
    );
    rx.await.context("background task failed")?;

    Ok(())
}

#[derive(Debug)]
struct InternalError;
impl warp::reject::Reject for InternalError {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args: Args = Clap::parse();

    let args = Arc::new(args);
    let port = args.listen_port;

    let route = warp::post()
        .and(warp::path("debug"))
        .and(warp::filters::body::json())
        .and_then(move |data| {
            let args = args.clone();
            async move {
                match handler(data, args).await {
                    Ok(()) => Ok("ok"),
                    Err(err) => {
                        tracing::error!(error = %format_args!("{:#}", err));
                        Err(warp::reject::custom(InternalError))
                    }
                }
            }
        });

    warp::serve(route).bind(([0, 0, 0, 0], port)).await;

    Ok(())
}
