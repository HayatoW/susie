use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

const METRICS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/evidence/metrics.jsonl"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub startup_ms: u64,
    pub fps: u32,
}

pub struct Sampler {
    started: Instant,
    first: Option<Instant>,
    frames: u32,
    done: bool,
}

impl Sampler {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            first: None,
            frames: 0,
            done: false,
        }
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    /// 最初のフレームから 1 秒未満に出したフレームを数える。境界のフレームは数に入れない。
    pub fn observe(&mut self, now: Instant) -> Option<Sample> {
        if self.done {
            return None;
        }
        let Some(first) = self.first else {
            self.first = Some(now);
            self.frames = 1;
            return None;
        };
        if now.duration_since(first) < Duration::from_secs(1) {
            self.frames += 1;
            return None;
        }
        self.done = true;
        let startup_ms = u64::try_from(first.duration_since(self.started).as_millis())
            .expect("起動時間は u64 に収まる");
        Some(Sample {
            startup_ms,
            fps: self.frames,
        })
    }
}

pub fn metrics_line(sample: Sample) -> String {
    format!(
        "{{\"startup_ms\":{},\"fps\":{}}}",
        sample.startup_ms, sample.fps
    )
}

pub fn append_sample(sample: Sample) -> std::io::Result<()> {
    let path = Path::new(METRICS_PATH);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", metrics_line(sample))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_records_startup_ms_and_fps() {
        let line = metrics_line(Sample {
            startup_ms: 40,
            fps: 2,
        });
        assert_eq!(line, "{\"startup_ms\":40,\"fps\":2}");
    }

    #[test]
    fn one_second_keeps_frames_before_the_boundary() {
        let started = Instant::now();
        let mut sampler = Sampler::new(started);
        let first = started + Duration::from_millis(40);
        assert!(sampler.observe(first).is_none());
        assert!(
            sampler
                .observe(first + Duration::from_millis(500))
                .is_none()
        );
        let sample = sampler
            .observe(first + Duration::from_secs(1))
            .expect("the boundary closes the sample");
        assert_eq!(
            sample,
            Sample {
                startup_ms: 40,
                fps: 2
            }
        );
        assert!(sampler.finished());
        assert!(
            sampler
                .observe(first + Duration::from_millis(1500))
                .is_none()
        );
    }
}
