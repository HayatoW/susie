use std::sync::Arc;

use engine::CpuFrame;
use gpui::RenderImage;
use image::{Frame, RgbaImage};

pub fn copied_image(frame: &CpuFrame) -> Arc<RenderImage> {
    let bytes = frame.bgra().to_vec();
    let buffer = RgbaImage::from_raw(frame.width(), frame.height(), bytes)
        .expect("画素バッファの長さは幅と高さと一致する");
    Arc::new(RenderImage::new(vec![Frame::new(buffer)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_keeps_bgra_bytes() {
        let frame = CpuFrame::copy_bgra(1, 1, &[1, 2, 3, 4]).unwrap();
        let image = copied_image(&frame);
        assert_eq!(image.as_bytes(0), Some(&[1, 2, 3, 4][..]));
    }
}
