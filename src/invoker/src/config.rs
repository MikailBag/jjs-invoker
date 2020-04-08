use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct InvokerConfig {
    /// How many workers should be spawned
    /// By default equal to processor count
    #[serde(default)]
    pub workers: Option<usize>,
    /// Configures how much invoker will sleep between ticks
    #[serde(default)]
    pub sleep: SleepConfig,
}
#[derive(Serialize, Deserialize)]
pub struct SleepConfig {
    /// Max sleep duration
    #[serde(default = "SleepConfig::default_max")]
    pub max_ms: u32,
    /// Growth of sleep duration if tick had not any updates
    #[serde(default = "SleepConfig::default_step")]
    pub step_ms: u32,
}

impl SleepConfig {
    fn default_max() -> u32 {
        2000
    }

    fn default_step() -> u32 {
        500
    }
}

impl Default for SleepConfig {
    fn default() -> Self {
        SleepConfig {
            max_ms: Self::default_max(),
            step_ms: Self::default_step(),
        }
    }
}
