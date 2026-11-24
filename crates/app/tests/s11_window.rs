#![cfg(target_os = "linux")]

mod harness;

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness::{
    App, Keyboard, MetricsFile, Pointer, Recording, Sway, Word, assert_playable, capture,
    extract_frame, find_containing, publish, recognize_block, transcode,
};

const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S11");
const OAK_URL: &str = "file:///tmp/fromoak.html";
const ELM_URL: &str = "file:///tmp/fromelm.html";
const OPEN_DEADLINE: Duration = Duration::from_secs(120);
const TYPE_DEADLINE: Duration = Duration::from_secs(30);
const NAV_DEADLINE: Duration = Duration::from_secs(45);
const HOLD: Duration = Duration::from_millis(1200);

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S11"]
fn keys_go_back_forward_and_reload() {
    let _metrics = MetricsFile::save();
    write_page("/tmp/fromoak.html", "susie-oak", "susie-marked");
    write_page("/tmp/fromelm.html", "susie-elm", "susie-stamped");
    let sway = Sway::start("s11");
    let mut app = App::spawn_with(&sway, &[OAK_URL]);
    let shot_path = shot("s11-screen.png");
    wait_until(&sway, &mut app, &shot_path, OPEN_DEADLINE, "oak", |text| {
        text.contains("fromoak") && text.contains("susie-oak") && !text.contains("marked")
    });
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    click_word(&sway, &shot_path, "press");
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "the marked oak page",
        |text| text.contains("susie-marked") && text.contains("fromoak"),
    );

    let mut recording = Recording::start(&sway);
    thread::sleep(Duration::from_millis(500));
    let origin = Instant::now();
    let mut keyboard = Keyboard::start(&sway);
    thread::sleep(Duration::from_millis(300));
    sway.focus_window(app.pid());

    keyboard.alt_left();
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "oak still marked",
        |text| text.contains("susie-marked") && !text.contains("susie-elm"),
    );
    let blocked_back = hold_mark(origin);

    click_bar(&sway, &shot_path, "fromoak");
    sway.focus_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    keyboard.type_text(ELM_URL);
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        TYPE_DEADLINE,
        "the typed elm url",
        |text| {
            text.contains("fromelm") && text.contains("susie-marked") && !text.contains("fromoak")
        },
    );
    thread::sleep(Duration::from_millis(200));
    keyboard.press_enter();
    wait_until(&sway, &mut app, &shot_path, NAV_DEADLINE, "elm", |text| {
        text.contains("susie-elm") && text.contains("fromelm") && !text.contains("susie-oak")
    });
    let opened_elm = hold_mark(origin);

    click_word(&sway, &shot_path, "press");
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "the stamped elm page",
        |text| text.contains("susie-stamped") && text.contains("fromelm"),
    );
    sway.focus_window(app.pid());
    thread::sleep(Duration::from_millis(200));
    keyboard.alt_right();
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "elm still stamped",
        |text| text.contains("susie-stamped") && !text.contains("susie-oak"),
    );
    let blocked_forward = hold_mark(origin);

    sway.focus_window(app.pid());
    keyboard.alt_left();
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "oak again",
        |text| {
            text.contains("susie-oak")
                && text.contains("fromoak")
                && !text.contains("susie-elm")
                && !text.contains("marked")
        },
    );
    let went_back = hold_mark(origin);

    sway.focus_window(app.pid());
    keyboard.alt_right();
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "elm again",
        |text| {
            text.contains("susie-elm")
                && text.contains("fromelm")
                && !text.contains("susie-oak")
                && !text.contains("stamped")
        },
    );
    let went_forward = hold_mark(origin);

    click_word(&sway, &shot_path, "press");
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "elm stamped before reload",
        |text| text.contains("susie-stamped") && text.contains("fromelm"),
    );
    sway.focus_window(app.pid());
    thread::sleep(Duration::from_millis(200));
    keyboard.ctrl_r();
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "elm reloaded",
        |text| {
            text.contains("susie-elm")
                && text.contains("fromelm")
                && !text.contains("stamped")
                && !text.contains("susie-oak")
        },
    );
    let reloaded = hold_mark(origin);

    sway.focus_window(app.pid());
    keyboard.ctrl_l();
    thread::sleep(Duration::from_millis(300));
    keyboard.type_text("qqqq");
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        TYPE_DEADLINE,
        "the selected url replaced",
        replaced_the_url,
    );
    let focused = hold_mark(origin);

    let raw = recording.stop();
    let video = shot("s11-navigate.mp4");
    transcode(&raw, &video);
    assert_playable(&video, 8.0, "navigation keys");
    assert_scene(&video, blocked_back, "blocked back", |text| {
        text.contains("susie-marked") && !text.contains("susie-elm")
    });
    assert_scene(&video, opened_elm, "opened elm", |text| {
        text.contains("susie-elm") && text.contains("fromelm") && !text.contains("susie-oak")
    });
    assert_scene(&video, blocked_forward, "blocked forward", |text| {
        text.contains("susie-stamped") && !text.contains("susie-oak")
    });
    assert_scene(&video, went_back, "went back", |text| {
        text.contains("susie-oak") && !text.contains("susie-elm") && !text.contains("marked")
    });
    assert_scene(&video, went_forward, "went forward", |text| {
        text.contains("susie-elm") && !text.contains("susie-oak") && !text.contains("stamped")
    });
    assert_scene(&video, reloaded, "reloaded", |text| {
        text.contains("susie-elm") && text.contains("fromelm") && !text.contains("stamped")
    });
    assert_scene(&video, focused, "address focus", replaced_the_url);
    publish(&video, &Path::new(EVIDENCE_DIR).join("navigate.mp4"));
    close_cleanly(&sway, &mut app);
}

fn replaced_the_url(text: &str) -> bool {
    text.contains("susie-elm")
        && text.contains('q')
        && !text.contains("fromelm")
        && !text.contains("html")
}

fn hold_mark(origin: Instant) -> f64 {
    thread::sleep(Duration::from_millis(350));
    let at = origin.elapsed().as_secs_f64() + 0.5;
    thread::sleep(HOLD);
    at
}

fn assert_scene(video: &Path, at: f64, what: &str, ready: impl Fn(&str) -> bool) {
    let frame = shot(&format!("s11-{}.png", what.replace(' ', "-")));
    let seconds = format!("{at:.3}");
    extract_frame(video, &["-ss", &seconds], &frame);
    let text = flat(&recognize_block(&frame));
    assert!(
        ready(&text),
        "{what} was not in the recording at {at:.2}s\n{text}"
    );
}

fn click_bar(sway: &Sway, shot_path: &Path, needle: &str) {
    let bar = click_word(sway, shot_path, needle);
    assert!(
        bar.top < 400,
        "{needle} was not in the address bar: {bar:?}"
    );
}

fn click_word(sway: &Sway, shot_path: &Path, needle: &str) -> Word {
    capture(sway, shot_path);
    let word = find_containing(shot_path, needle)
        .unwrap_or_else(|| panic!("{needle} was not on screen\n{}", recognize_block(shot_path)));
    sway.place_cursor(word.center());
    thread::sleep(Duration::from_millis(200));
    let mut pointer = Pointer::start(sway);
    thread::sleep(Duration::from_millis(200));
    pointer.click();
    thread::sleep(Duration::from_millis(400));
    sway.place_cursor(harness::Spot { x: 1100, y: 700 });
    word
}

fn wait_until(
    sway: &Sway,
    app: &mut App,
    shot_path: &Path,
    deadline_after: Duration,
    what: &str,
    ready: impl Fn(&str) -> bool,
) {
    let deadline = Instant::now() + deadline_after;
    let mut placed = false;
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited before {what} was visible\n{}",
            app.stderr()
        );
        if !sway.has_window(app.pid()) {
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        if !placed {
            sway.place_window(app.pid());
            placed = true;
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        capture(sway, shot_path);
        let text = flat(&recognize_block(shot_path));
        if ready(&text) {
            return;
        }
        last = text;
        thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "{what} was not visible within {}s\n{last}\n{}\n{}",
        deadline_after.as_secs(),
        app.stderr(),
        sway.stderr()
    );
}

fn write_page(path: &str, heading: &str, marked: &str) {
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  html, body {{ margin: 0; background: #ffffff; color: #111111; font-family: "Noto Sans", sans-serif; }}
  h1 {{ font-size: 96px; line-height: 1.1; margin: 0; padding: 24px 32px; }}
  button {{ margin: 0 32px; font: inherit; font-size: 48px; padding: 8px 16px; }}
</style>
</head>
<body>
  <h1 id="title">{heading}</h1>
  <button id="click" type="button" onclick="document.getElementById('title').textContent='{marked}'">press</button>
</body>
</html>
"#
    );
    fs::write(path, html).expect("write the page");
}

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn close_cleanly(sway: &Sway, app: &mut App) {
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

fn shot(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}
