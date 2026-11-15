//! 人の操作なしで計測行を `docs/evidence/metrics.jsonl` に追記する検証用バイナリ。

use std::env;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use susie_metrics::{append_metrics_line, MetricsRecorder};

fn default_evidence_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("../../docs/evidence/metrics.jsonl")
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(manifest_dir).join("../../docs/evidence/metrics.jsonl")
        })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_evidence_path);

    let mut recorder = MetricsRecorder::new();
    // 起動から最初のフレームまでの待ちを模擬する。
    thread::sleep(Duration::from_millis(25));
    recorder.on_frame();

    let frame_interval = Duration::from_micros(16_666);
    let end = std::time::Instant::now() + Duration::from_secs(1);
    while std::time::Instant::now() < end {
        recorder.on_frame();
        thread::sleep(frame_interval);
    }

    let line = recorder.metrics()?;
    append_metrics_line(&path, &line)?;
    println!(
        "wrote startup_ms={} fps={} to {}",
        line.startup_ms,
        line.fps,
        path.display()
    );
    Ok(())
}