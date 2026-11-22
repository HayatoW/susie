#![cfg(target_os = "linux")]

mod harness;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness::{
    App, MetricsFile, Pointer, Recording, SCREEN_HEIGHT, SCREEN_WIDTH, Spot, Sway, Wheel,
    assert_playable, capture, extract_frame, input_ms, metric_lines, publish, recognize,
    require_word, transcode, wait_until_ready,
};

const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S13");
const CLICK_DEADLINE: Duration = Duration::from_secs(30);

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S13"]
fn click_records_input_ms() {
    let mut metrics = MetricsFile::save();
    let sway = Sway::start("s13");
    let mut app = App::spawn(&sway);
    let before = metric_lines();
    let top = shot("s13-top.png");
    wait_until_ready(&sway, &mut app, before, &top, "click");
    assert!(
        input_ms().is_none(),
        "input_ms was recorded before the page press"
    );
    // 最初の configure が遅れて届くと、ウィンドウが出力いっぱいに戻る。
    // そのあとで置き直すと、クリックのあいだサイズが保たれる。
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    let mut recording = Recording::start(&sway);
    thread::sleep(Duration::from_millis(500));
    click_once(&sway, &mut app);
    thread::sleep(Duration::from_millis(700));
    let raw = recording.stop();
    let video = shot("s13-click.mp4");
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
    assert!(
        input_ms().is_some(),
        "the launch dropped input_ms before the window closed"
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
    metrics.keep_new_line();
}

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF"]
fn scroll_after_a_blank_press_does_not_record_input_ms() {
    let _metrics = MetricsFile::save();
    let sway = Sway::start("s13-scroll");
    let mut app = App::spawn(&sway);
    let before = metric_lines();
    let shot_path = shot("s13-blank.png");
    wait_until_ready(&sway, &mut app, before, &shot_path, "blank");
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    press_blank(&sway, &shot_path);
    thread::sleep(Duration::from_millis(500));
    assert!(
        input_ms().is_none(),
        "a blank press recorded input_ms before the scroll"
    );
    let mut wheel = start_wheel(&sway);
    scroll_until_bottom(&sway, &mut app, &mut wheel);
    thread::sleep(Duration::from_millis(500));
    assert!(input_ms().is_none(), "the scroll was recorded as input_ms");
    capture(&sway, &shot_path);
    let text = recognize(&shot_path);
    assert!(
        !text.contains("susie-clicked"),
        "the blank press changed the page: {text:?}"
    );
    sway.close_window(app.pid());
    let status = app.wait_for_exit();
    assert_eq!(
        status.code(),
        Some(0),
        "susie did not exit 0: {status}\n{}",
        app.stderr()
    );
    assert!(
        input_ms().is_none(),
        "input_ms was recorded after the window closed"
    );
}

fn press_blank(sway: &Sway, shot_path: &Path) {
    let top = require_word(sway, shot_path, "susie-top");
    let spot = Spot {
        x: top.left + top.width + 120,
        y: top.top + top.height / 2,
    };
    assert!(
        spot.x < i32::try_from(SCREEN_WIDTH).expect("screen width fits in i32") - 20,
        "the blank spot is outside the screen: {spot:?} {top:?}"
    );
    sway.place_cursor(spot);
    thread::sleep(Duration::from_millis(200));
    let mut pointer = Pointer::start(sway);
    thread::sleep(Duration::from_millis(200));
    pointer.click();
}

fn start_wheel(sway: &Sway) -> Wheel {
    sway.place_cursor(Spot {
        x: i32::try_from(SCREEN_WIDTH / 2).expect("screen width fits in i32"),
        y: i32::try_from(SCREEN_HEIGHT / 2).expect("screen height fits in i32"),
    });
    thread::sleep(Duration::from_millis(200));
    let wheel = Wheel::start(sway);
    thread::sleep(Duration::from_millis(200));
    wheel
}

fn scroll_until_bottom(sway: &Sway, app: &mut App, wheel: &mut Wheel) {
    let deadline = Instant::now() + CLICK_DEADLINE;
    let shot_path = shot("s13-bottom.png");
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited while scrolling\n{}",
            app.stderr()
        );
        assert!(
            input_ms().is_none(),
            "input_ms was recorded while scrolling\n{last}"
        );
        wheel.scroll(200);
        thread::sleep(Duration::from_millis(250));
        capture(sway, &shot_path);
        let text = recognize(&shot_path);
        if text.contains("susie-bottom") {
            return;
        }
        last = text;
    }
    panic!(
        "susie-bottom was not visible within {}s\n{last}\n{}",
        CLICK_DEADLINE.as_secs(),
        app.stderr()
    );
}

fn click_once(sway: &Sway, app: &mut App) {
    let deadline = Instant::now() + CLICK_DEADLINE;
    let shot_path = shot("s13-click.png");
    // 押し直すと、最初の押下から画素が変わるまでの時間が伸びる。1 回だけ押す。
    let spot = require_word(sway, &shot_path, "press").center();
    sway.place_cursor(spot);
    thread::sleep(Duration::from_millis(200));
    let mut pointer = Pointer::start(sway);
    // enter が GPUI に届いてからボタンを送る。先に送ると、面に入る前のボタンは捨てられる。
    thread::sleep(Duration::from_millis(200));
    assert!(
        input_ms().is_none(),
        "input_ms was recorded before the press was sent"
    );
    pointer.click();
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited while clicking\n{}",
            app.stderr()
        );
        thread::sleep(Duration::from_millis(200));
        capture(sway, &shot_path);
        let text = recognize(&shot_path);
        if text.contains("susie-clicked") && input_ms().is_some() {
            return;
        }
        last = format!("input_ms {:?} text {text}", input_ms());
    }
    panic!(
        "susie-clicked and input_ms were not both recorded within {}s\n{last}\n{}",
        CLICK_DEADLINE.as_secs(),
        app.stderr()
    );
}

fn ends_of(video: &Path) -> (PathBuf, PathBuf) {
    let early = shot("s13-early.png");
    let late = shot("s13-late.png");
    extract_frame(video, &["-ss", "0.2"], &early);
    extract_frame(video, &["-sseof", "-0.3"], &late);
    (early, late)
}

fn shot(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}
