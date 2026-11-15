use std::path::Path;
use std::sync::Arc;

/// 最初の表示が使うビューの幅。フィクスチャのスペーサはこれより高い。
pub const VIEW_WIDTH: u32 = 800;

/// 最初の表示が使うビューの高さ。フィクスチャのスペーサはこれより高い。
pub const VIEW_HEIGHT: u32 = 600;

/// CEF の OnPaint が渡す BGRA を、コールバックの外でも持つ画素。
///
/// バッファはコールバックが戻ると無効になる。ここへコピーしたあとの所有者は呼び出し側である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuFrame {
    width: u32,
    height: u32,
    bgra: Arc<[u8]>,
}

/// フレームの受け渡し。今は CPU の画素だけを持つ。DMA-BUF は後から別のバリアントとして足す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Cpu(CpuFrame),
}

#[derive(Debug)]
pub enum FrameError {
    Empty,
    BufferLength { expected: usize, actual: usize },
    Png(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Empty => write!(f, "フレームの幅か高さが 0 です"),
            FrameError::BufferLength { expected, actual } => write!(
                f,
                "画素バッファは {actual} バイトです。{expected} バイト必要です"
            ),
            FrameError::Png(message) => write!(f, "PNG を書けません。{message}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl CpuFrame {
    /// `src` は BGRA で、長さは `width * height * 4` と一致する。
    pub fn copy_bgra(width: u32, height: u32, src: &[u8]) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::Empty);
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(FrameError::Empty)?;
        if src.len() != expected {
            return Err(FrameError::BufferLength {
                expected,
                actual: src.len(),
            });
        }
        Ok(Self {
            width,
            height,
            bgra: Arc::from(src),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bgra(&self) -> &[u8] {
        &self.bgra
    }
}

pub fn write_png(frame: &CpuFrame, path: &Path) -> Result<(), FrameError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| FrameError::Png(err.to_string()))?;
        }
    }
    let mut rgba = Vec::with_capacity(frame.bgra().len());
    for pixel in frame.bgra().chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    let image = image::RgbaImage::from_raw(frame.width(), frame.height(), rgba)
        .ok_or(FrameError::Empty)?;
    image
        .save(path)
        .map_err(|err| FrameError::Png(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_rejects_a_short_buffer() {
        let err = CpuFrame::copy_bgra(2, 2, &[0, 0, 0, 255]).unwrap_err();
        assert!(matches!(
            err,
            FrameError::BufferLength {
                expected: 16,
                actual: 4
            }
        ));
    }

    #[test]
    fn copy_keeps_the_bytes() {
        let frame = CpuFrame::copy_bgra(1, 1, &[1, 2, 3, 4]).unwrap();
        assert_eq!(frame.width(), 1);
        assert_eq!(frame.height(), 1);
        assert_eq!(frame.bgra(), &[1, 2, 3, 4]);
    }

    #[test]
    fn png_roundtrip_keeps_a_red_pixel() {
        let frame = CpuFrame::copy_bgra(1, 1, &[0, 0, 255, 255]).unwrap();
        let path = std::env::temp_dir().join(format!("susie-frame-{}.png", std::process::id()));
        write_png(&frame, &path).unwrap();
        let loaded = image::load_from_memory(&std::fs::read(&path).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(loaded.get_pixel(0, 0).0, [255, 0, 0, 255]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn fixture_keeps_the_bottom_outside_the_view() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/first-page.html");
        let html = std::fs::read_to_string(path).unwrap();
        assert!(html.contains("susie-top"));
        assert!(html.contains("susie-bottom"));
        assert!(html.contains("susie-clicked"));
        assert!(html.contains("<button"));
        assert!(html.contains("<input"));
        let marker = "data-susie-spacer=\"";
        let start = html.find(marker).unwrap() + marker.len();
        let end = html[start..].find('"').unwrap();
        let spacer: u32 = html[start..start + end].parse().unwrap();
        assert!(spacer > VIEW_HEIGHT);
        assert!(VIEW_WIDTH > 0);
    }
}
