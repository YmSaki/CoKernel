use cokernel_metrics::Sampler;
use std::time::Duration;
#[tokio::main]
async fn main() {
    let sampler = Sampler::start(std::env::current_dir().expect("working directory"));
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let snapshot = sampler.snapshot();
            if snapshot.sequence >= 2 {
                assert!(!snapshot.stale && snapshot.collecting);
                let resources = snapshot.resources.as_ref().unwrap();
                println!("{}", serde_json::to_string(&snapshot).unwrap());
                let memory = resources.memory.value.as_ref().expect("real OS memory");
                assert!(
                    memory.total_bytes > 0
                        && memory.used_bytes + memory.available_bytes == memory.total_bytes
                );
                assert!(
                    resources
                        .storage
                        .value
                        .as_ref()
                        .expect("real filesystem capacity")
                        .total_bytes
                        > 0
                );
                assert!(
                    resources
                        .cpu_percent
                        .value
                        .is_some_and(|v| (0.0..=100.0).contains(&v)),
                    "{:?}",
                    resources.cpu_percent
                );
                if std::env::args().any(|arg| arg == "--require-gpu") {
                    assert!(
                        !resources
                            .gpus
                            .value
                            .as_ref()
                            .expect("real NVIDIA diagnostic")
                            .is_empty()
                    );
                }
                for _ in 0..64 {
                    assert_eq!(sampler.snapshot().sequence, snapshot.sequence);
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    sampler.shutdown().await.expect("collector reaped");
    result.expect("two bounded real samples");
    assert!(sampler.snapshot().stale);
    println!("PASS: real OS/GPU metrics, cached snapshots, explicit shutdown");
}
