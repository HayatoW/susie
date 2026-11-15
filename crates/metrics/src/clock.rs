use std::time::{Duration, Instant};

use crate::Sample;

const WINDOW: Duration = Duration::from_secs(1);

/// 起動時間と、その最初の 1 秒に出したフレーム数を集めます。
#[derive(Clone, Debug)]
pub struct FrameClock {
    state: State,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Waiting {
        origin: Instant,
    },
    Counting {
        startup_ms: u64,
        first: Instant,
        frames: u32,
        last: Instant,
    },
    Done(Sample),
}

/// [`FrameClock::record_frame`] が返せる、計測の途中経過です。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// 最初のフレームは見えていて、 1 秒はまだ終わっていません。
    Counting { startup_ms: u64, frames: u32 },
    /// 1 秒が終わりました。 `sample.fps` はそのあいだのフレーム数です。
    Done(Sample),
}

/// フレーム時刻が計測の規則に合わないときの失敗です。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockError {
    /// フレームがプロセス開始より前です。
    FrameBeforeOrigin,
    /// フレームの時刻が、直前のフレームより前です。
    FrameOutOfOrder,
    /// 最初のフレームの前に、計測を閉じようとしました。
    NotStarted,
    /// 最初のフレームから 1 秒が経つ前に、計測を閉じようとしました。
    WindowStillOpen,
    /// 1 秒のフレーム数が `u32` を超えました。
    FrameCountOverflow,
    /// 起動時間が `u64` ミリ秒を超えました。
    DurationOverflow,
}

impl std::fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            ClockError::FrameBeforeOrigin => "フレームがプロセス開始より前です",
            ClockError::FrameOutOfOrder => "フレームの時刻が戻っています",
            ClockError::NotStarted => "最初のフレームの前に計測を閉じています",
            ClockError::WindowStillOpen => "最初のフレームから 1 秒が経っていません",
            ClockError::FrameCountOverflow => "1 秒のフレーム数が u32 を超えました",
            ClockError::DurationOverflow => "起動時間が u64 ミリ秒を超えました",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ClockError {}

impl FrameClock {
    /// プロセス開始時刻 `origin` で、最初のフレームを待っている時計を作ります。
    pub fn waiting(origin: Instant) -> Self {
        Self {
            state: State::Waiting { origin },
        }
    }

    /// 時刻 `at` にフレームを 1 枚出したことを記録します。
    ///
    /// 最初のフレームで `startup_ms` が決まります。値は `origin` から `at` までのミリ秒で、
    /// 1 ミリ秒未満は切り捨てます。
    /// 続くフレームは `[最初のフレーム, 最初のフレーム + 1 秒)` に入るときだけ数えます。
    /// ちょうど 1 秒後以降のフレームは数えずに、計測を閉じます。
    /// 閉じたあとに再び呼んでも、同じ [`Sample`] のままです。
    pub fn record_frame(&mut self, at: Instant) -> Result<Progress, ClockError> {
        match self.state {
            State::Waiting { origin } => {
                let startup_ms = elapsed_ms(origin, at)?;
                self.state = State::Counting {
                    startup_ms,
                    first: at,
                    frames: 1,
                    last: at,
                };
                Ok(Progress::Counting {
                    startup_ms,
                    frames: 1,
                })
            }
            State::Counting {
                startup_ms,
                first,
                frames,
                last,
            } => {
                if at < last {
                    return Err(ClockError::FrameOutOfOrder);
                }
                if at.saturating_duration_since(first) >= WINDOW {
                    let sample = Sample {
                        startup_ms,
                        fps: frames,
                    };
                    self.state = State::Done(sample);
                    return Ok(Progress::Done(sample));
                }
                let frames = frames
                    .checked_add(1)
                    .ok_or(ClockError::FrameCountOverflow)?;
                self.state = State::Counting {
                    startup_ms,
                    first,
                    frames,
                    last: at,
                };
                Ok(Progress::Counting { startup_ms, frames })
            }
            State::Done(sample) => Ok(Progress::Done(sample)),
        }
    }

    /// フレームを足さずに、時刻 `at` で計測を閉じます。
    ///
    /// `at` は最初のフレームから 1 秒以上後である必要があります。
    /// 閉じたあとに再び呼んでも、同じ [`Sample`] を返します。
    pub fn finish(&mut self, at: Instant) -> Result<Sample, ClockError> {
        match self.state {
            State::Waiting { .. } => Err(ClockError::NotStarted),
            State::Counting {
                startup_ms,
                first,
                frames,
                ..
            } => {
                if at < first {
                    return Err(ClockError::FrameOutOfOrder);
                }
                if at.saturating_duration_since(first) < WINDOW {
                    return Err(ClockError::WindowStillOpen);
                }
                let sample = Sample {
                    startup_ms,
                    fps: frames,
                };
                self.state = State::Done(sample);
                Ok(sample)
            }
            State::Done(sample) => Ok(sample),
        }
    }
}

fn elapsed_ms(origin: Instant, at: Instant) -> Result<u64, ClockError> {
    let duration = at
        .checked_duration_since(origin)
        .ok_or(ClockError::FrameBeforeOrigin)?;
    u64::try_from(duration.as_millis()).map_err(|_| ClockError::DurationOverflow)
}
