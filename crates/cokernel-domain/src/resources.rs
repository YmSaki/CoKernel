//! Resource observations preserve unavailable values and OS scope on the wire.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Unavailable {
    WarmingUp,
    NotSupported,
    NotInstalled,
    ReadFailed,
    InvalidData,
    TimedOut,
    CollectorFailed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metric<T> {
    pub value: Option<T>,
    pub unavailable: Option<Unavailable>,
}
impl<T> Metric<T> {
    pub fn available(value: T) -> Self {
        Self {
            value: Some(value),
            unavailable: None,
        }
    }
    pub fn unavailable(reason: Unavailable) -> Self {
        Self {
            value: None,
            unavailable: Some(reason),
        }
    }
}
impl<T> From<Result<T, Unavailable>> for Metric<T> {
    fn from(result: Result<T, Unavailable>) -> Self {
        match result {
            Ok(value) => Self::available(value),
            Err(reason) => Self::unavailable(reason),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memory {
    pub total_bytes: u64,
    /// Windows: physical total - available; Linux: MemTotal - MemAvailable.
    pub used_bytes: u64,
    pub available_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Storage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    /// Available to this identity; can differ from total - used due to reserves/quotas.
    pub available_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CgroupMemory {
    pub current_bytes: u64,
    /// Minimum finite memory.max over this cgroup and its ancestors; null = unlimited.
    pub limit_bytes: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gpu {
    pub uuid: String,
    pub name: String,
    pub utilization_percent: Metric<f64>,
    pub memory_used_bytes: Metric<u64>,
    pub memory_total_bytes: Metric<u64>,
    pub temperature_celsius: Metric<f64>,
    pub power_watts: Metric<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resources {
    /// A source boundary, not a claim that Windows and Linux values are additive.
    pub source: String,
    pub sampled_at: String,
    pub cpu_percent: Metric<f64>,
    pub memory: Metric<Memory>,
    pub storage: Metric<Storage>,
    pub cgroup_memory: Metric<CgroupMemory>,
    pub gpus: Metric<Vec<Gpu>>,
    /// Attribution is not inferred from global VRAM usage, especially under WDDM/WSL.
    pub process_gpu_attribution: Metric<()>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub sequence: u64,
    pub age_ms: Option<u64>,
    pub stale: bool,
    pub collecting: bool,
    pub resources: Option<Resources>,
}
