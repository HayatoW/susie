//! プロセス開始から最初のフレームまでの起動時間と、最初のフレームから 1 秒間の fps を記録する。

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// `docs/evidence/metrics.jsonl` の 1 行分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsLine {
    pub startup_ms: u64,
    pub fps: u32,
}

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("まだ 1 フレームも出ていないため計測値がない")]
    NoMetrics,
}

/// フレーム通知から起動時間と fps を集計する。
#[derive(Debug)]
pub struct MetricsRecorder {
    process_start: Instant,
    first_frame: Option<Instant>,
    fps_window_end: Option<Instant>,
    frame_count_in_window: u32,
    startup_ms: Option<u64>,
}

impl MetricsRecorder {
    /// いまをプロセス開始として計測を始める。
    pub fn new() -> Self {
        Self::with_process_start(Instant::now())
    }

    /// テストや統合時に、すでに決まっている開始時刻を使う。
    pub fn with_process_start(process_start: Instant) -> Self {
        Self {
            process_start,
            first_frame: None,
            fps_window_end: None,
            frame_count_in_window: 0,
            startup_ms: None,
        }
    }

    /// 描画フレームが 1 枚出たときに呼ぶ。
    pub fn on_frame(&mut self) {
        let now = Instant::now();
        match self.first_frame {
            None => {
                self.first_frame = Some(now);
                self.startup_ms = Some(
                    now.duration_since(self.process_start)
                        .as_millis()
                        .try_into()
                        .unwrap_or(u64::MAX),
                );
                self.fps_window_end = Some(now + Duration::from_secs(1));
                self.frame_count_in_window = 1;
            }
            Some(first) => {
                let Some(end) = self.fps_window_end else {
                    return;
                };
                if now <= end {
                    self.frame_count_in_window += 1;
                }
                let _ = first;
            }
        }
    }

    /// 少なくとも 1 フレームがあれば計測行を返す。
    pub fn metrics(&self) -> Result<MetricsLine, MetricsError> {
        let startup_ms = self
            .startup_ms
            .ok_or(MetricsError::NoMetrics)?;
        Ok(MetricsLine {
            startup_ms,
            fps: self.frame_count_in_window,
        })
    }
}

impl Default for MetricsRecorder {
    fn default() -> Self {
        Self::new()
    }
}

/// 計測行を JSONL の末尾に 1 行追記する。
pub fn append_metrics_line(path: impl AsRef<Path>, line: &MetricsLine) -> Result<(), MetricsError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(line)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{json}")?;
    Ok(())
}

/// ファイルの最終行を計測行として読む。空なら `None`。
pub fn read_last_metrics_line(path: impl AsRef<Path>) -> Result<Option<MetricsLine>, MetricsError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut last: Option<String> = None;
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            last = Some(trimmed.to_owned());
        }
    }
    match last {
        None => Ok(None),
        Some(text) => Ok(Some(serde_json::from_str(&text)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn append_and_read_last_roundtrip() {
        let path = std::env::temp_dir().join(format!(
            "susie-metrics-test-{}-metrics.jsonl",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let line = MetricsLine {
            startup_ms: 120,
            fps: 58,
        };
        append_metrics_line(&path, &line).expect("append");
        let read = read_last_metrics_line(&path)
            .expect("read")
            .expect("some line");
        assert_eq!(read, line);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn recorder_startup_and_fps_window() {
        let process_start = Instant::now();
        thread::sleep(Duration::from_millis(15));
        let mut rec = MetricsRecorder::with_process_start(process_start);
        rec.on_frame();
        let after_first = Instant::now();
        while after_first.elapsed() < Duration::from_millis(500) {
            rec.on_frame();
            thread::sleep(Duration::from_millis(8));
        }
        let m = rec.metrics().expect("metrics");
        assert!(m.startup_ms >= 10);
        assert!(m.fps >= 2);
    }
}