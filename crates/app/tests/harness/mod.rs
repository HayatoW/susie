//! ヘッドレス Sway でウィンドウを開き、仮想デバイスで操作し、録画する。
//! S06 と S07 と S08 と S09 と S12 と S13 が、この 1 か所を使う。
//! 各テストが使う操作は違うので、使わない項目は未使用になる。
#![allow(dead_code)]

mod input;
mod media;
mod session;

// 再エクスポートは、そのテストが使わない名前でも未使用になる。
#[allow(unused_imports)]
pub use input::{Keyboard, Pointer, Wheel};
#[allow(unused_imports)]
pub use media::{
    Recording, Word, assert_playable, duration_secs, extract_frame, field_has, field_spot,
    find_containing, publish, recognize, recognize_block, require_word, transcode,
    wait_until_ready,
};
#[allow(unused_imports)]
pub use session::{
    App, MetricsFile, SCREEN_HEIGHT, SCREEN_WIDTH, Spot, Sway, capture, input_ms_since,
    metric_lines,
};
