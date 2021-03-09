use crate::{
    handler::Handler,
    shim::{ShimClient, ShimResponse},
};
use anyhow::Context;
use std::{convert::Infallible, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};
use warp::Filter;

#[derive(Debug, Clone)]
pub enum ListenAddress {
    Tcp(SocketAddr),
    Uds(PathBuf),
}

impl FromStr for ListenAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let u = url::Url::parse(s).context("invalid url")?;
        match u.scheme() {
            "tcp" => {
                let addr = u.host_str().context("listen address missing")?;
                let ip = addr.parse().context("listen address is not IP")?;
                let port = u.port().context("listen port missing")?;
                let addr = SocketAddr::new(ip, port);
                Ok(ListenAddress::Tcp(addr))
            }
            "unix" => {
                let path = u.path().into();
                Ok(ListenAddress::Uds(path))
            }
            other => anyhow::bail!("unknown scheme {}, expected one of 'tcp', 'unix'", other),
        }
    }
}

type Resp = hyper::Response<hyper::Body>;

async fn route_exec_inner(
    handler: Arc<Handler>,
    shim: Arc<ShimClient>,
    req: serde_json::Value,
) -> anyhow::Result<Resp> {
    let shim_response = shim
        .call(req)
        .await
        .context("failed to preprocess request using shim")?;

    let req = match shim_response {
        ShimResponse::Accept(r) => r,
        ShimResponse::Reject(rej) => {
            let response = serde_json::json!({
                "error": "request rejected by the shim",
                "details": rej
            });
            let response = serde_json::to_string(&response)?;
            return Ok(hyper::Response::builder()
                .status(400)
                .body(response.into())
                .expect("incorrect response"));
        }
    };
    let req = serde_json::from_value(req).context("incorrect InvokeRequest")?;

    let response = handler.handle_invoke_request(&req).await?;
    let response = serde_json::to_vec(&response).context("failed to serialize InvokeResponse")?;

    Ok(hyper::Response::builder()
        .status(200)
        .body(response.into())
        .expect("incorrect response"))
}

/// Handler for /exec requests
#[tracing::instrument(skip(handler, shim, req))]
async fn route_exec(
    handler: Arc<Handler>,
    shim: Arc<ShimClient>,
    req: serde_json::Value,
) -> Result<Resp, Infallible> {
    let res = route_exec_inner(handler, shim, req).await;
    match res {
        Ok(response) => Ok(response),
        Err(err) => {
            let error_id = uuid::Uuid::new_v4();
            let err = format!("{:#}", err);

            tracing::error!(error = %err, error_id = %error_id.to_hyphenated(), "invocation request failed");

            Ok(hyper::Response::builder()
                .status(500)
                .header("Error-UUID", error_id.to_hyphenated().to_string())
                .body((&[] as &'static [u8]).into())
                .expect("incorrect response"))
        }
    }
}

/// Handler for /ready requests
async fn route_ready() -> Result<&'static str, Infallible> {
    Ok("OK")
}

/// Server HTTP API.
pub struct Server {
    handler: Arc<Handler>,
    shim: Arc<ShimClient>,
}

impl Server {
    pub fn new(handler: Handler, shim: ShimClient) -> Self {
        Server {
            handler: Arc::new(handler),
            shim: Arc::new(shim),
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn serve(self, addr: ListenAddress) -> anyhow::Result<()> {
        let handler = self.handler.clone();
        let shim = self.shim.clone();
        let r_exec = warp::path("exec")
            .and(warp::filters::body::json())
            .and_then(move |req| route_exec(handler.clone(), shim.clone(), req));
        let r_ready = warp::path("ready").and_then(route_ready);
        #[cfg(debug_assertions)]
        let r_exec = r_exec.boxed();
        #[cfg(debug_assertions)]
        let r_ready = r_ready.boxed();

        let srv = r_exec.or(r_ready);
        let srv = warp::serve(srv);
        match addr {
            ListenAddress::Tcp(addr) => {
                srv.run(addr).await;
            }
            ListenAddress::Uds(path) => {
                let listener = tokio::net::UnixListener::bind(&path)
                    .with_context(|| format!("failed to attach to UDS {}", path.display()))?;
                let listener = tokio_stream::wrappers::UnixListenerStream::new(listener);
                srv.run_incoming(listener).await;
            }
        }

        Ok(())
    }
}
