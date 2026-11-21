#![cfg(target_os = "linux")]

mod harness;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness::{
    App, MetricsFile, Pointer, Recording, Spot, Sway, assert_playable, capture, extract_frame,
    metric_lines, publish, recognize, require_word, transcode, wait_until_ready,
};

const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S06");
const CLICK_DEADLINE: Duration = Duration::from_secs(30);

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S06"]
fn click_shows_susie_clicked() {
    let _metrics = MetricsFile::save();
    let sway = Sway::start("s06");
    let mut app = App::spawn(&sway);
    let before = metric_lines();
    let top = shot("s06-top.png");
    wait_until_ready(&sway, &mut app, before, &top, "click");
    // 最初の configure が遅れて届くと、ウィンドウが出力いっぱいに戻る。
    // そのあとで置き直すと、クリックのあいだサイズが保たれる。
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    let mut recording = Recording::start(&sway);
    thread::sleep(Duration::from_millis(500));
    click_until_marked(&sway, &mut app);
    thread::sleep(Duration::from_millis(700));
    let raw = recording.stop();
    let video = shot("s06-click.mp4");
    transcode(&raw, &video);
    assert_playable(&video, 1.0, "click");
    let (early, late) = ends_of(&video);
    let early_text = recognize(&early);
    let late_text = recognize(&late);
    assert!(
        early_text.contains("susie-top") && !early_text.contains("susie-clicked"),
        "the recording did not start before the click: {early_text:?}"
    );
    assert!(
        late_text.contains("susie-clicked"),
        "the recording did not show susie-clicked: {late_text:?}"
    );
    publish(&video, &Path::new(EVIDENCE_DIR).join("click.mp4"));
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

fn click_until_marked(sway: &Sway, app: &mut App) {
    let deadline = Instant::now() + CLICK_DEADLINE;
    let shot_path = shot("s06-click.png");
    let spot = press_spot(sway, &shot_path);
    sway.place_cursor(spot);
    thread::sleep(Duration::from_millis(200));
    let mut pointer = Pointer::start(sway);
    // enter が GPUI に届いてからボタンを送る。先に送ると、面に入る前のボタンは捨てられる。
    thread::sleep(Duration::from_millis(200));
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited while clicking\n{}",
            app.stderr()
        );
        let spot = press_spot(sway, &shot_path);
        sway.place_cursor(spot);
        thread::sleep(Duration::from_millis(150));
        pointer.click();
        thread::sleep(Duration::from_millis(400));
        capture(sway, &shot_path);
        let text = recognize(&shot_path);
        if text.contains("susie-clicked") {
            return;
        }
        last = format!("{spot:?} {text}");
    }
    panic!(
        "susie-clicked was not visible within {}s\n{last:?}\n{}",
        CLICK_DEADLINE.as_secs(),
        app.stderr()
    );
}

fn press_spot(sway: &Sway, shot_path: &Path) -> Spot {
    require_word(sway, shot_path, "press").center()
}

fn ends_of(video: &Path) -> (PathBuf, PathBuf) {
    let early = shot("s06-early.png");
    let late = shot("s06-late.png");
    extract_frame(video, &["-ss", "0.2"], &early);
    extract_frame(video, &["-sseof", "-0.3"], &late);
    (early, late)
}

fn shot(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}
