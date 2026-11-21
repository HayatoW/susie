#![cfg(target_os = "linux")]

mod harness;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use harness::{
    App, Keyboard, MetricsFile, Pointer, Recording, SCREEN_HEIGHT, SCREEN_WIDTH, Spot, Sway, Wheel,
    assert_playable, capture, duration_secs, field_has, field_spot, metric_lines, publish,
    recognize, require_word, transcode, wait_until_ready,
};

const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S08");
const SCROLL_DEADLINE: Duration = Duration::from_secs(30);
const CLICK_DEADLINE: Duration = Duration::from_secs(30);
const TYPE_DEADLINE: Duration = Duration::from_secs(30);
const HOLD: Duration = Duration::from_secs(2);

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S08"]
fn one_launch_shows_scroll_click_and_typing() {
    let mut metrics = MetricsFile::save();
    let sway = Sway::start("s08");
    let mut app = App::spawn(&sway);
    let before = metric_lines();
    let top = shot("s08-top.png");
    wait_until_ready(&sway, &mut app, before, &top, "scroll");
    // 最初の configure が遅れて届くと、ウィンドウが出力いっぱいに戻る。
    // そのあとで置き直すと、スクロールとクリックと打鍵のあいだサイズが保たれる。
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    let mut recording = Recording::start(&sway);
    thread::sleep(Duration::from_millis(500));

    let mut wheel = start_wheel(&sway);
    scroll_until(&sway, &mut app, &mut wheel, true);
    // 下端の文字が録画の中で読めるあいだ止める。
    thread::sleep(HOLD);
    scroll_until(&sway, &mut app, &mut wheel, false);
    // スクロールのポインタを消してから、クリックのポインタを作る。
    drop(wheel);

    click_until_marked(&sway, &mut app);
    thread::sleep(HOLD);
    type_until_shown(&sway, &mut app);
    thread::sleep(HOLD);

    let raw = recording.stop();
    let video = shot("s08-steps.mp4");
    transcode(&raw, &video);
    assert_playable(&video, 4.0, "scroll, click, and typing");
    assert_stage_order(&video);
    publish(&video, &Path::new(EVIDENCE_DIR).join("steps.mp4"));
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

fn start_wheel(sway: &Sway) -> Wheel {
    sway.place_cursor(Spot {
        x: i32::try_from(SCREEN_WIDTH / 2).expect("screen width fits in i32"),
        y: i32::try_from(SCREEN_HEIGHT / 2).expect("screen height fits in i32"),
    });
    thread::sleep(Duration::from_millis(200));
    let wheel = Wheel::start(sway);
    // enter が GPUI に届いてから軸を送る。先に送ると、面に入る前の軸は捨てられる。
    thread::sleep(Duration::from_millis(200));
    wheel
}

fn scroll_until(sway: &Sway, app: &mut App, wheel: &mut Wheel, down: bool) {
    let deadline = Instant::now() + SCROLL_DEADLINE;
    let shot_path = shot(if down {
        "s08-bottom.png"
    } else {
        "s08-top-again.png"
    });
    let step = if down { 400 } else { -400 };
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited while scrolling\n{}",
            app.stderr()
        );
        wheel.scroll(step);
        thread::sleep(Duration::from_millis(250));
        capture(sway, &shot_path);
        let text = recognize(&shot_path);
        let reached = if down {
            text.contains("susie-bottom")
        } else {
            text.contains("susie-top")
                && text.contains("press")
                && !text.contains("susie-bottom")
                && !text.contains("susie-clicked")
        };
        if reached {
            return;
        }
        last = text;
    }
    let edge = if down { "susie-bottom" } else { "susie-top" };
    panic!(
        "{edge} was not visible within {}s\n{last:?}\n{}",
        SCROLL_DEADLINE.as_secs(),
        app.stderr()
    );
}

fn click_until_marked(sway: &Sway, app: &mut App) {
    let deadline = Instant::now() + CLICK_DEADLINE;
    let shot_path = shot("s08-click.png");
    let spot = require_word(sway, &shot_path, "press").center();
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
        let spot = require_word(sway, &shot_path, "press").center();
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

fn type_until_shown(sway: &Sway, app: &mut App) {
    let deadline = Instant::now() + TYPE_DEADLINE;
    let shot_path = shot("s08-field.png");
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
            return;
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

fn assert_stage_order(video: &Path) {
    let dir = shot("s08-frames");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).expect("create the frame dir");
    let pattern = dir.join("frame-%03d.png");
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            video.to_str().expect("video path is utf-8"),
            "-vf",
            "fps=2",
            pattern.to_str().expect("frame pattern is utf-8"),
        ])
        .output()
        .expect("sample recording frames");
    assert!(
        output.status.success(),
        "ffmpeg could not sample frames\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut frames: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read sampled frames")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .collect();
    frames.sort();
    assert!(!frames.is_empty(), "the recording produced no frames");

    let mut top_at = None;
    let mut bottom_at = None;
    let mut clicked_at = None;
    let mut typed_at = None;
    let mut log = String::new();
    for (index, frame) in frames.iter().enumerate() {
        let text = recognize(frame);
        let top = text.contains("susie-top");
        let bottom = text.contains("susie-bottom");
        let clicked = text.contains("susie-clicked");
        let field = clicked_at.is_some() && field_has(frame, "susie");
        let name = frame
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        log.push_str(&format!(
            "{name} top {top} bottom {bottom} clicked {clicked} field {field}\n"
        ));
        if top_at.is_none() && top && !bottom && !clicked {
            top_at = Some(index);
        }
        if top_at.is_some() && bottom_at.is_none() && bottom && !clicked {
            bottom_at = Some(index);
        }
        if bottom_at.is_some() && clicked_at.is_none() && clicked && !field_has(frame, "susie") {
            clicked_at = Some(index);
        }
        if clicked_at.is_some_and(|clicked_index| index > clicked_index) && field {
            typed_at = Some(index);
            break;
        }
    }
    let (Some(top_at), Some(bottom_at), Some(clicked_at), Some(typed_at)) =
        (top_at, bottom_at, clicked_at, typed_at)
    else {
        panic!(
            "the recording did not show susie-top, then susie-bottom, then susie-clicked, then susie in the field\nduration {}s\n{log}",
            duration_secs(video)
        );
    };
    assert!(
        top_at < bottom_at && bottom_at < clicked_at && clicked_at < typed_at,
        "the stages were not in order: top {top_at}, bottom {bottom_at}, clicked {clicked_at}, typed {typed_at}\n{log}"
    );
}

fn shot(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}
