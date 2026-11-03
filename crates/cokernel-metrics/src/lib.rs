//! Bounded, read-only OS sampling shared by Host and Runtime; no worker/domain authority.
mod gpu;
mod platform;
pub use cokernel_domain::resources::*;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{sync::watch, time::Instant};

const ACTIVE_INTERVAL: Duration = Duration::from_secs(2);
const BACKGROUND_INTERVAL: Duration = Duration::from_secs(10);
const ACTIVE_WINDOW: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug)]
struct CpuCounters {
    total: u64,
    idle: u64,
}
fn cpu_delta(previous: Option<CpuCounters>, current: CpuCounters) -> Metric<f64> {
    let Some(previous) = previous else {
        return Metric::unavailable(Unavailable::WarmingUp);
    };
    let Some(total) = current.total.checked_sub(previous.total).filter(|n| *n > 0) else {
        return Metric::unavailable(Unavailable::InvalidData);
    };
    let Some(idle) = current
        .idle
        .checked_sub(previous.idle)
        .filter(|n| *n <= total)
    else {
        return Metric::unavailable(Unavailable::InvalidData);
    };
    Metric::available(100.0 * (total - idle) as f64 / total as f64)
}
struct Collector {
    storage: PathBuf,
    previous_cpu: Option<CpuCounters>,
}
impl Collector {
    fn collect_os(mut self) -> (Self, Resources) {
        let sampled_at = chrono::Utc::now().to_rfc3339();
        let cpu = match platform::cpu() {
            Ok(current) => {
                let value = cpu_delta(self.previous_cpu, current);
                self.previous_cpu = Some(current);
                value
            }
            Err(reason) => {
                self.previous_cpu = None;
                Metric::unavailable(reason)
            }
        };
        let resources = Resources {
            source: if cfg!(windows) {
                "WINDOWS_HOST"
            } else {
                "LINUX_RUNTIME"
            }
            .into(),
            sampled_at,
            cpu_percent: cpu,
            memory: platform::memory().into(),
            storage: platform::storage(&self.storage).into(),
            cgroup_memory: platform::cgroup_memory().into(),
            gpus: Metric::unavailable(Unavailable::WarmingUp),
            process_gpu_attribution: Metric::unavailable(Unavailable::NotSupported),
        };
        (self, resources)
    }
}
#[derive(Clone)]
struct Sample {
    sequence: u64,
    observed: Instant,
    resources: Resources,
}
struct Inner {
    value: watch::Receiver<Option<Sample>>,
    activity: watch::Sender<Instant>,
    stop: watch::Sender<bool>,
    done: watch::Receiver<bool>,
}
/// Clones share one sampler and at most one OS/GPU collection in flight.
#[derive(Clone)]
pub struct Sampler(Arc<Inner>);
impl Sampler {
    pub fn start(storage: PathBuf) -> Self {
        Self::start_with_gpu(storage, gpu::sample)
    }
    fn start_with_gpu<F, Fut>(storage: PathBuf, mut gpu_sample: F) -> Self
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Vec<Gpu>, Unavailable>> + Send,
    {
        let (value_tx, value) = watch::channel(None);
        let (activity, mut active) = watch::channel(Instant::now());
        let (stop, mut stopping) = watch::channel(false);
        let (done_tx, done) = watch::channel(false);
        tokio::spawn(async move {
            let mut collector = Collector {
                storage,
                previous_cpu: None,
            };
            let mut sequence = 0u64;
            loop {
                let observed = Instant::now();
                // OS APIs/files may block, but neither Runtime admission nor the Tokio reactor does.
                let sampled = tokio::task::spawn_blocking(move || collector.collect_os()).await;
                let Ok((next, mut resources)) = sampled else {
                    break;
                };
                collector = next;
                resources.gpus = gpu_sample().await.into();
                sequence = sequence.saturating_add(1);
                value_tx.send_replace(Some(Sample {
                    sequence,
                    observed,
                    resources,
                }));
                if *stopping.borrow() || stopping.has_changed().is_err() {
                    break;
                }
                let interval = if active.borrow().elapsed() < ACTIVE_WINDOW {
                    ACTIVE_INTERVAL
                } else {
                    BACKGROUND_INTERVAL
                };
                let next = Instant::now() + interval;
                let earliest = Instant::now() + ACTIVE_INTERVAL;
                tokio::select! {
                    biased;
                    _ = stopping.changed() => {done_tx.send_replace(true); return;}
                    _ = tokio::time::sleep_until(next) => {},
                    changed = active.changed() => {
                        if changed.is_err() {done_tx.send_replace(true); return;}
                        // Demand can shorten background sleep, never start a second collector.
                        tokio::select! { _ = stopping.changed()=>{done_tx.send_replace(true);return;}, _=tokio::time::sleep_until(earliest)=>{} }
                    }
                }
            }
            done_tx.send_replace(true);
        });
        Self(Arc::new(Inner {
            value,
            activity,
            stop,
            done,
        }))
    }
    pub fn snapshot(&self) -> Snapshot {
        self.0.activity.send_replace(Instant::now());
        let sample = self.0.value.borrow().clone();
        let collecting = !*self.0.done.borrow() && self.0.done.has_changed().is_ok();
        match sample {
            None => Snapshot {
                sequence: 0,
                age_ms: None,
                stale: true,
                collecting,
                resources: None,
            },
            Some(sample) => {
                let age = sample.observed.elapsed();
                Snapshot {
                    sequence: sample.sequence,
                    age_ms: Some(age.as_millis().min(u64::MAX as u128) as u64),
                    stale: !collecting || age > BACKGROUND_INTERVAL + Duration::from_secs(5),
                    collecting,
                    resources: Some(sample.resources),
                }
            }
        }
    }
    pub async fn shutdown(&self) -> Result<(), &'static str> {
        self.0.stop.send_replace(true);
        let mut done = self.0.done.clone();
        tokio::time::timeout(Duration::from_secs(4), async {
            while !*done.borrow() {
                if done.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .map_err(|_| "Resource collector cleanup is still pending")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn slow_collector_does_not_block_snapshots_or_multiply_work_and_shutdown_joins_it() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let sampler = Sampler::start_with_gpu(std::env::temp_dir(), {
            let entered = entered.clone();
            let release = release.clone();
            let calls = calls.clone();
            move || {
                let entered = entered.clone();
                let release = release.clone();
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    entered.notify_one();
                    release.notified().await;
                    Err(Unavailable::TimedOut)
                }
            }
        });
        tokio::time::timeout(Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        let clone = sampler.clone();
        for _ in 0..1024 {
            assert_eq!(clone.snapshot().sequence, 0);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let stopping = tokio::spawn({
            let sampler = sampler.clone();
            async move { sampler.shutdown().await }
        });
        tokio::task::yield_now().await;
        assert!(
            !stopping.is_finished(),
            "shutdown acknowledged while diagnostic work was still owned"
        );
        release.notify_one();
        stopping.await.unwrap().unwrap();
        let snapshot = sampler.snapshot();
        assert_eq!(snapshot.sequence, 1);
        assert!(snapshot.stale && !snapshot.collecting);
        assert_eq!(
            snapshot.resources.unwrap().gpus.unavailable,
            Some(Unavailable::TimedOut)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn last_owner_drop_stops_collection() {
        let sampler = Sampler::start_with_gpu(std::env::temp_dir(), || async { Ok(vec![]) });
        let mut done = sampler.0.done.clone();
        drop(sampler);
        tokio::time::timeout(Duration::from_secs(3), async {
            while !*done.borrow() {
                done.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
    #[test]
    fn cpu_uses_deltas_and_rejects_reset_zero_and_invalid_idle() {
        let first = CpuCounters {
            total: 100,
            idle: 40,
        };
        assert_eq!(
            cpu_delta(None, first).unavailable,
            Some(Unavailable::WarmingUp)
        );
        assert_eq!(
            cpu_delta(
                Some(first),
                CpuCounters {
                    total: 200,
                    idle: 65
                }
            )
            .value,
            Some(75.0)
        );
        for bad in [
            first,
            CpuCounters {
                total: 99,
                idle: 40,
            },
            CpuCounters {
                total: 101,
                idle: 43,
            },
        ] {
            assert_eq!(
                cpu_delta(Some(first), bad).unavailable,
                Some(Unavailable::InvalidData)
            );
        }
    }
}
