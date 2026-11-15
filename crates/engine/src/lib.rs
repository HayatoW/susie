//! 描画元から受け取った画素を、後段へ渡す。

mod frame;

pub use frame::{write_png, CpuFrame, Frame, FrameError, VIEW_HEIGHT, VIEW_WIDTH};
