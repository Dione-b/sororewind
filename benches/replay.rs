//! Offline replay benchmarks over every recorded fixture.

#![allow(missing_docs, clippy::unwrap_used, clippy::print_stderr)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::Value;
use sororewind::pipeline::{InspectStage, benchmark_stage};
use sororewind::{BoxFuture, FixtureTransport, Request, Result, RpcClient, Transport};

struct Counting {
    fixture: FixtureTransport,
    calls: Arc<AtomicUsize>,
}

impl Transport for Counting {
    fn request<'a>(&'a self, method: &'a str, params: &'a Value) -> BoxFuture<'a, Result<String>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.fixture.request(method, params)
    }

    fn origin(&self) -> String {
        self.fixture.origin()
    }
}

fn benchmark(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut dirs: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("fixture.json").is_file())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy();
        let fixture = runtime.block_on(FixtureTransport::open(&dir)).unwrap();
        let request = Request {
            hash: fixture.hash_for(None).unwrap(),
            network: None,
            wasm_debug: None,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let rpc = RpcClient::new(Counting {
            fixture,
            calls: Arc::clone(&calls),
        });
        runtime.block_on(sororewind::run(&rpc, &request)).unwrap();
        eprintln!(
            "{name}: {} RPC calls per full replay",
            calls.load(Ordering::Relaxed)
        );

        let mut group = c.benchmark_group(name.as_ref());
        for (name, stage) in [
            ("decode", InspectStage::Decode),
            ("reconstruct", InspectStage::State),
            ("replay", InspectStage::Replay),
        ] {
            group.bench_function(name, |b| {
                b.iter_custom(|iterations| {
                    runtime.block_on(async {
                        let mut measured = Duration::ZERO;
                        for _ in 0..iterations {
                            let times = benchmark_stage(&rpc, &request, stage).await.unwrap();
                            measured += times;
                        }
                        measured
                    })
                })
            });
        }
        group.finish();
    }
}

criterion_group! {
    name = offline;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(300))
        .measurement_time(Duration::from_secs(1))
        .noise_threshold(0.15);
    targets = benchmark
}
criterion_main!(offline);
