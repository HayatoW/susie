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

pub fn input_line(input_ms: u64) -> String {
    format!("{{\"input_ms\":{input_ms}}}")
}

/// ページへ渡した最初の左ボタンの押下から、画素が変わったフレームを載せた時刻まで。
/// 1 回の起動では最初の押下だけを覚える。アドレスバーのクリックは、ここへ来ない。
#[derive(Debug)]
pub struct InputLatency {
    pressed: Option<Instant>,
    // 画素が変わったフレームを、次の present で閉じる。
    pending: bool,
    done: bool,
}

impl InputLatency {
    pub fn new() -> Self {
        Self {
            pressed: None,
            pending: false,
            done: false,
        }
    }

    /// 最初の押下だけを覚える。覚えたときだけ true。
    pub fn press(&mut self, now: Instant) -> bool {
        if self.pressed.is_some() || self.done {
            return false;
        }
        self.pressed = Some(now);
        true
    }

    /// 押下のあとで画素が変わったフレームなら、次の present で閉じる。
    pub fn see(&mut self, changed: bool) -> bool {
        if self.done || self.pending || self.pressed.is_none() || !changed {
            return false;
        }
        self.pending = true;
        true
    }

    /// 画素が変わる前の別操作で、未確定の押下を捨てる。捨てたときだけ true。
    /// その起動では、あとの押下も測らない。変わったフレームを待っている計測は残す。
    pub fn cancel(&mut self) -> bool {
        if self.done || self.pending || self.pressed.is_none() {
            return false;
        }
        self.pressed = None;
        self.done = true;
        true
    }

    /// 変わったフレームを GPUI に載せた時刻で、押下からのミリ秒を返す。
    pub fn present(&mut self, now: Instant) -> Option<u64> {
        if !self.pending {
            return None;
        }
        self.pending = false;
        self.done = true;
        let start = self.pressed.take()?;
        let elapsed = u64::try_from(now.duration_since(start).as_millis())
            .expect("入力の遅れは u64 に収まる");
        Some(elapsed)
    }
}

pub fn append_sample(sample: Sample) -> std::io::Result<()> {
    append_line(&metrics_line(sample))
}

pub fn append_input_ms(input_ms: u64) -> std::io::Result<()> {
    append_line(&input_line(input_ms))
}

fn append_line(line: &str) -> std::io::Result<()> {
    let path = Path::new(METRICS_PATH);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
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

    #[test]
    fn line_records_input_ms() {
        assert_eq!(input_line(25), "{\"input_ms\":25}");
    }

    #[test]
    fn first_press_closes_when_the_changed_frame_is_presented() {
        let start = Instant::now();
        let mut latency = InputLatency::new();
        assert!(latency.press(start));
        assert!(!latency.press(start + Duration::from_millis(30)));
        assert!(!latency.see(false));
        assert!(latency.present(start + Duration::from_millis(10)).is_none());
        assert!(latency.see(true));
        assert!(!latency.see(true));
        assert_eq!(latency.present(start + Duration::from_millis(40)), Some(40));
        assert!(latency.present(start + Duration::from_millis(80)).is_none());
        assert!(!latency.press(start + Duration::from_millis(90)));
        assert!(!latency.see(true));
    }

    #[test]
    fn change_before_the_press_is_not_input() {
        let start = Instant::now();
        let mut latency = InputLatency::new();
        assert!(!latency.see(true));
        assert!(latency.present(start).is_none());
    }

    #[test]
    fn cancel_before_a_changed_frame_drops_the_press() {
        let start = Instant::now();
        let mut latency = InputLatency::new();
        assert!(latency.press(start));
        assert!(latency.cancel());
        assert!(!latency.cancel());
        assert!(!latency.see(true));
        assert!(latency.present(start + Duration::from_secs(3)).is_none());
        assert!(!latency.press(start + Duration::from_secs(4)));
    }

    #[test]
    fn cancel_after_the_changed_frame_keeps_the_press() {
        let start = Instant::now();
        let mut latency = InputLatency::new();
        assert!(latency.press(start));
        assert!(latency.see(true));
        assert!(!latency.cancel());
        assert_eq!(latency.present(start + Duration::from_millis(40)), Some(40));
    }
}
