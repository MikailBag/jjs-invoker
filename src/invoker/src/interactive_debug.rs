use rand::Rng;
use std::{path::PathBuf, time::Instant};

/// Suspender is used to wait until user has connected to the sandbox.
pub struct Suspender(Mode);

enum Mode {
    None,
    Dir(PathBuf),
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
        Suspender(Mode::None)
    }

    pub(crate) async fn suspend(&self, data: serde_json::Value) -> anyhow::Result<()> {
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
        }
        let elapsed = begin.elapsed();
        tracing::info!(elapsed = %elapsed.as_secs_f32(), "sandbox resumed");
        Ok(())
    }
}
