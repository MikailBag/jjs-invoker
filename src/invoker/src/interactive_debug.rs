use anyhow::Context;
use invoker_api::debug::AttachRequest;
use rand::Rng;
use std::{path::PathBuf, time::Instant};

/// Suspender is used to wait until user has connected to the sandbox.
pub struct Suspender(Mode);

enum Mode {
    None,
    Dir(PathBuf),
    Http(String, reqwest::Client),
}

fn generate_token() -> String {
    rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .map(|x| x as char)
        .take(10)
        .collect()
}

impl Suspender {
    pub(crate) fn new(args: &crate::CliArgs) -> Self {
        if let Some(p) = &args.interactive_debug_path {
            return Suspender(Mode::Dir(p.clone()));
        }
        if let Some(url) = &args.interactive_debug_url {
            return Suspender(Mode::Http(url.clone(), reqwest::Client::new()));
        }
        Suspender(Mode::None)
    }

    pub(crate) async fn suspend(&self, data: AttachRequest) -> anyhow::Result<()> {
        let begin = Instant::now();
        match &self.0 {
            Mode::None => {}
            Mode::Dir(dir) => {
                let data = serde_json::to_string_pretty(&data)?;
                let path = dir.join(generate_token());
                tracing::warn!(
                    data = data.as_str(),
                    path = %path.display(),
                    "sandbox is suspended until path is touched"
                );
                loop {
                    if tokio::fs::metadata(&path).await.is_ok() {
                        break;
                    }
                }
            }
            Mode::Http(url, client) => {
                client
                    .post(url)
                    .json(&data)
                    .send()
                    .await
                    .context("failed to connect to interactive debugging webhook")?
                    .error_for_status()
                    .context("interactive debugger webhook failed")?;
            }
        }
        let elapsed = begin.elapsed();
        tracing::info!(elapsed = %elapsed.as_secs_f32(), "sandbox resumed");
        Ok(())
    }
}
