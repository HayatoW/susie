#![cfg(target_os = "linux")]

mod harness;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness::{
    App, Keyboard, MetricsFile, Pointer, Recording, Sway, assert_playable, capture, extract_frame,
    field_has, field_spot, metric_lines, publish, recognize, recognize_block, require_word,
    transcode, wait_until_ready,
};

const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S07");
const TYPE_DEADLINE: Duration = Duration::from_secs(30);

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S07"]
fn field_shows_susie() {
    let _metrics = MetricsFile::save();
    let sway = Sway::start("s07");
    let mut app = App::spawn(&sway);
    let before = metric_lines();
    let top = shot("s07-top.png");
    wait_until_ready(&sway, &mut app, before, &top, "typing");
    // 最初の configure が遅れて届くと、ウィンドウが出力いっぱいに戻る。
    // そのあとで置き直すと、クリックと打鍵のあいだサイズが保たれる。
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    let mut recording = Recording::start(&sway);
    thread::sleep(Duration::from_millis(500));
    let mut keyboard = type_until_shown(&sway, &mut app);
    thread::sleep(Duration::from_millis(700));
    let raw = recording.stop();
    let video = shot("s07-type.mp4");
    transcode(&raw, &video);
    assert_playable(&video, 1.0, "typing");
    let (early, late) = ends_of(&video);
    let early_text = recognize(&early);
    assert!(
        early_text.contains("susie-top") && !field_has(&early, "susie"),
        "the recording did not start before the typing: {early_text:?}"
    );
    assert!(
        field_has(&late, "susie"),
        "the recording did not show susie in the field: {}",
        recognize(&late)
    );
    publish(&video, &Path::new(EVIDENCE_DIR).join("type.mp4"));
    // 録画のあとに記号を打つ。録画は susie だけのまま、削除やカーソル移動を確かめる。
    assert_symbols_stay(&sway, &mut app, &mut keyboard);
    assert!(
        app.is_running(),
        "susie exited before the window closed\n{}",
        app.stderr()
    );
    sway.close_window(app.pid());
    let status = app.wait_for_exit();
    assert_eq!(
        status.code(),
        Some(0),
        "susie did not exit 0: {status}\n{}",
        app.stderr()
    );
}

fn type_until_shown(sway: &Sway, app: &mut App) -> Keyboard {
    let deadline = Instant::now() + TYPE_DEADLINE;
    let shot_path = shot("s07-field.png");
    let press = require_word(sway, &shot_path, "press");
    sway.place_cursor(field_spot(&press));
    thread::sleep(Duration::from_millis(200));
    let mut pointer = Pointer::start(sway);
    // enter が GPUI に届いてからボタンを送る。先に送ると、面に入る前のボタンは捨てられる。
    thread::sleep(Duration::from_millis(200));
    pointer.click();
    thread::sleep(Duration::from_millis(400));
    sway.focus_window(app.pid());
    let mut keyboard = Keyboard::start(sway);
    // キーマップがウィンドウへ届いてから打つ。
    thread::sleep(Duration::from_millis(300));
    keyboard.type_text("susie");
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited while typing\n{}",
            app.stderr()
        );
        capture(sway, &shot_path);
        if field_has(&shot_path, "susie") {
            return keyboard;
        }
        last = recognize(&shot_path);
        thread::sleep(Duration::from_millis(300));
    }
    panic!(
        "susie was not in the field within {}s\n{last}\n{}",
        TYPE_DEADLINE.as_secs(),
        app.stderr()
    );
}

fn assert_symbols_stay(sway: &Sway, app: &mut App, keyboard: &mut Keyboard) {
    let shot_path = shot("s07-symbols.png");
    keyboard.type_text(".$%");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited while typing symbols\n{}",
            app.stderr()
        );
        thread::sleep(Duration::from_millis(300));
        capture(sway, &shot_path);
        let text = recognize_block(&shot_path);
        let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        // `.` が Delete、 `$` が Home、 `%` が Left だと、この並びにはならない。
        if compact.contains("susie.$%") {
            return;
        }
        last = text;
    }
    panic!(
        "the field did not keep susie and the symbols\n{last}\n{}",
        app.stderr()
    );
}

fn ends_of(video: &Path) -> (PathBuf, PathBuf) {
    let early = shot("s07-early.png");
    let late = shot("s07-late.png");
    extract_frame(video, &["-ss", "0.2"], &early);
    extract_frame(video, &["-sseof", "-0.3"], &late);
    (early, late)
}

fn shot(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}
