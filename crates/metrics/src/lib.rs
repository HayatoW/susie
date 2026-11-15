//! 起動時間と描画の fps を 1 行ずつ残す。
//!
//! 起動時間はプロセス開始から最初のフレームまでのミリ秒で、フィールド名は `startup_ms` です。
//! 1 ミリ秒未満は切り捨てます。
//! 描画の fps は、最初のフレームから 1 秒経つまでに出したフレームの数で、フィールド名は `fps` です。
//! 数える範囲は最初のフレームを含み、ちょうど 1 秒後の時刻は含みません。
//! 1 件は [`EVIDENCE_RELATIVE_PATH`] の 1 行です。

mod clock;

pub use clock::{ClockError, FrameClock, Progress};

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// ボードが指定する記録ファイルへの、リポジトリルートからの相対パスです。
pub const EVIDENCE_RELATIVE_PATH: &str = "docs/evidence/metrics.jsonl";

/// 1 回の起動で残す計測です。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub startup_ms: u64,
    pub fps: u32,
}

/// 記録ファイルの読み書きで起きる失敗です。
#[derive(Debug)]
pub enum Error {
    /// ファイルの作成、オープン、読み書きに失敗しました。
    Io(io::Error),
    /// `number` 行目を [`Sample`] にできませんでした。行番号は 1 からです。
    Line { number: usize, reason: LineError },
}

/// 1 行を [`Sample`] にできなかった理由です。
#[derive(Debug)]
pub enum LineError {
    /// 空白だけの行です。
    Empty,
    /// JSON として 2 つの整数フィールドを読めませんでした。
    Json(serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(source) => {
                write!(formatter, "記録ファイルの読み書きに失敗しました: {source}")
            }
            Error::Line {
                number,
                reason: LineError::Empty,
            } => write!(formatter, "{number} 行目が空です"),
            Error::Line {
                number,
                reason: LineError::Json(source),
            } => write!(
                formatter,
                "{number} 行目の JSON を読めませんでした: {source}"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(source) => Some(source),
            Error::Line {
                reason: LineError::Json(source),
                ..
            } => Some(source),
            Error::Line {
                reason: LineError::Empty,
                ..
            } => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(source: io::Error) -> Self {
        Error::Io(source)
    }
}

/// `sample` を `path` の末尾へ 1 行追加します。親ディレクトリがなければ作ります。
pub fn append_sample(path: &Path, sample: Sample) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let mut line =
        serde_json::to_string(&sample).expect("Sample は 2 つの整数なので JSON にできます");
    line.push('\n');

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// `path` の各行を、ファイルの順に読みます。ファイルが無いときはエラーです。
pub fn read_samples(path: &Path) -> Result<Vec<Sample>, Error> {
    let text = fs::read_to_string(path)?;
    parse_samples(&text)
}

fn parse_samples(text: &str) -> Result<Vec<Sample>, Error> {
    if text.is_empty() {
        return Ok(Vec::new());
    }

    let mut rows: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        rows.pop();
    }

    let mut samples = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let number = index + 1;
        let row = row.strip_suffix('\r').unwrap_or(row);
        if row.trim().is_empty() {
            return Err(Error::Line {
                number,
                reason: LineError::Empty,
            });
        }
        let sample = serde_json::from_str::<Sample>(row).map_err(|source| Error::Line {
            number,
            reason: LineError::Json(source),
        })?;
        samples.push(sample);
    }
    Ok(samples)
}
