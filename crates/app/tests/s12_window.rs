#![cfg(target_os = "linux")]

mod harness;

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness::{
    App, Keyboard, MetricsFile, Pointer, Recording, SCREEN_HEIGHT, SCREEN_WIDTH, Spot, Sway, Wheel,
    Word, assert_playable, capture, extract_frame, field_has, field_spot, find_containing, publish,
    recognize, recognize_block, require_word, transcode,
};

const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S12");
const FROM_URL: &str = "file:///tmp/frompage.html";
const TYPED_URL: &str = "file:///tmp/typedurl.html";
const PAGE_URL: &str = "file:///tmp/pane.html";
const FRAGMENT_URL: &str = "file:///tmp/pane.html#mark";
const OPEN_DEADLINE: Duration = Duration::from_secs(120);
const TYPE_DEADLINE: Duration = Duration::from_secs(30);
const NAV_DEADLINE: Duration = Duration::from_secs(45);
const SCROLL_DEADLINE: Duration = Duration::from_secs(30);

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S12"]
fn typed_url_opens_the_page() {
    let _metrics = MetricsFile::save();
    write_page("/tmp/frompage.html", "susie-from", false);
    write_page("/tmp/typedurl.html", "susie-dest", true);
    let sway = Sway::start("s12");
    let mut app = App::spawn_with(&sway, &[FROM_URL]);
    let shot_path = shot("s12-screen.png");
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        OPEN_DEADLINE,
        "the address bar and susie-from",
        |text| text.contains("frompage") && text.contains("susie-from"),
    );
    // 最初の configure が遅れて届くと、ウィンドウが出力いっぱいに戻る。
    // そのあとで置き直すと、クリックと打鍵のあいだサイズが保たれる。
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    let mut recording = Recording::start(&sway);
    thread::sleep(Duration::from_millis(500));
    let mut keyboard = type_url_and_enter(&sway, &mut app, &shot_path);
    wait_until(
        &sway,
        &mut app,
        &shot_path,
        NAV_DEADLINE,
        "susie-dest",
        |text| {
            text.contains("susie-dest") && text.contains("typedurl") && !text.contains("susie-from")
        },
    );
    thread::sleep(Duration::from_millis(1200));
    let raw = recording.stop();
    let video = shot("s12-open.mp4");
    transcode(&raw, &video);
    assert_playable(&video, 2.0, "address bar");
    let (early, late) = ends_of(&video);
    let early_text = flat(&recognize_block(&early));
    let late_text = flat(&recognize_block(&late));
    assert!(
        early_text.contains("susie-from") && !early_text.contains("susie-dest"),
        "the recording did not start before the navigation: {early_text:?}"
    );
    assert!(
        late_text.contains("typedurl") && late_text.contains("susie-dest"),
        "the recording did not show the typed url and the opened page: {late_text:?}"
    );
    // ページをクリックしたあとの文字は、入力欄ではなくページへ入る。
    type_into_the_page(&sway, &mut app, &mut keyboard, &shot_path);
    publish(&video, &Path::new(EVIDENCE_DIR).join("open.mp4"));
    close_cleanly(&sway, &mut app);
}

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S12"]
fn fragment_url_still_scrolls() {
    let _metrics = MetricsFile::save();
    write_tall("/tmp/pane.html");
    let sway = Sway::start("s12f");
    let mut app = App::spawn_with(&sway, &[PAGE_URL]);
    let shot_path = shot("s12f-screen.png");
    wait_until_line(
        &sway,
        &mut app,
        &shot_path,
        OPEN_DEADLINE,
        "the address bar and susie-top",
        |text| text.contains("pane") && text.contains("susie-top") && !text.contains("susie-low"),
    );
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    let mut recording = Recording::start(&sway);
    thread::sleep(Duration::from_millis(500));
    type_fragment_and_enter(&sway, &mut app, &shot_path);
    // フラグメントでは読み込み完了が来ない。ここで描画を止めると、このあとホイールが画面を変えない。
    thread::sleep(Duration::from_millis(400));
    let mut wheel = start_wheel(&sway);
    scroll_until_low(&sway, &mut app, &mut wheel);
    drop(wheel);
    sway.place_cursor(Spot { x: 1100, y: 700 });
    thread::sleep(Duration::from_millis(800));
    let raw = recording.stop();
    let video = shot("s12f-fragment.mp4");
    transcode(&raw, &video);
    assert_playable(&video, 2.0, "fragment scroll");
    let early = shot("s12f-early.png");
    let late = shot("s12f-late.png");
    extract_frame(&video, &["-ss", "0.2"], &early);
    extract_frame(&video, &["-sseof", "-0.3"], &late);
    let early_text = flat(&recognize(&early));
    let late_text = flat(&recognize(&late));
    assert!(
        early_text.contains("susie-top") && !early_text.contains("susie-low"),
        "the recording did not start on the top of the page: {early_text:?}"
    );
    assert!(
        late_text.contains("susie-low") && late_text.contains("mark"),
        "the recording did not show the fragment url and the scrolled page: {late_text:?}"
    );
    publish(&video, &Path::new(EVIDENCE_DIR).join("fragment.mp4"));
    close_cleanly(&sway, &mut app);
}

fn type_url_and_enter(sway: &Sway, app: &mut App, shot_path: &Path) -> Keyboard {
    let bar = click_word(sway, shot_path, "frompage");
    assert!(
        bar.top < 400,
        "frompage was not in the address bar: {bar:?}"
    );
    sway.focus_window(app.pid());
    let mut keyboard = Keyboard::start(sway);
    // キーマップがウィンドウへ届いてから打つ。
    thread::sleep(Duration::from_millis(300));
    keyboard.type_text(TYPED_URL);
    wait_until(
        sway,
        app,
        shot_path,
        TYPE_DEADLINE,
        "the typed url",
        |text| {
            text.contains("typedurl") && text.contains("susie-from") && !text.contains("frompage")
        },
    );
    thread::sleep(Duration::from_millis(200));
    keyboard.press_enter();
    keyboard
}

fn type_fragment_and_enter(sway: &Sway, app: &mut App, shot_path: &Path) {
    let bar = click_word(sway, shot_path, "pane");
    assert!(bar.top < 400, "pane was not in the address bar: {bar:?}");
    sway.focus_window(app.pid());
    let mut keyboard = Keyboard::start(sway);
    thread::sleep(Duration::from_millis(300));
    keyboard.type_text(FRAGMENT_URL);
    wait_until_line(
        sway,
        app,
        shot_path,
        TYPE_DEADLINE,
        "the fragment url",
        |text| text.contains("mark") && text.contains("susie-top") && !text.contains("susie-low"),
    );
    thread::sleep(Duration::from_millis(200));
    keyboard.press_enter();
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

fn scroll_until_low(sway: &Sway, app: &mut App, wheel: &mut Wheel) {
    let deadline = Instant::now() + SCROLL_DEADLINE;
    let shot_path = shot("s12f-low.png");
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited while scrolling\n{}",
            app.stderr()
        );
        wheel.scroll(200);
        thread::sleep(Duration::from_millis(250));
        capture(sway, &shot_path);
        let text = flat(&recognize(&shot_path));
        if text.contains("susie-low") {
            return;
        }
        last = text;
    }
    panic!(
        "susie-low was not visible within {}s\n{last}\n{}",
        SCROLL_DEADLINE.as_secs(),
        app.stderr()
    );
}

fn type_into_the_page(sway: &Sway, app: &mut App, keyboard: &mut Keyboard, shot_path: &Path) {
    capture(sway, shot_path);
    let press = require_word(sway, shot_path, "press");
    sway.place_cursor(field_spot(&press));
    thread::sleep(Duration::from_millis(200));
    let mut pointer = Pointer::start(sway);
    thread::sleep(Duration::from_millis(200));
    pointer.click();
    thread::sleep(Duration::from_millis(400));
    sway.focus_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    keyboard.type_text("hello");
    // ポインタが文字に重なると、読み取りが語を欠く。
    sway.place_cursor(Spot { x: 1100, y: 700 });
    thread::sleep(Duration::from_millis(200));
    let deadline = Instant::now() + TYPE_DEADLINE;
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited while typing into the page\n{}",
            app.stderr()
        );
        capture(sway, shot_path);
        let url = find_containing(shot_path, "typedurl");
        // hello はアドレスにも見出しにも無い。欄より下のページ欄にあれば、キーはページへ届いた。
        if url.is_some_and(|url| url.top < 400) && field_has(shot_path, "hello") {
            return;
        }
        last = flat(&recognize_block(shot_path));
        thread::sleep(Duration::from_millis(300));
    }
    panic!(
        "hello was not in the page below the address bar within {}s\n{last}\n{}",
        TYPE_DEADLINE.as_secs(),
        app.stderr()
    );
}

fn click_word(sway: &Sway, shot_path: &Path, needle: &str) -> Word {
    capture(sway, shot_path);
    let word = find_containing(shot_path, needle)
        .unwrap_or_else(|| panic!("{needle} was not on screen\n{}", recognize_block(shot_path)));
    sway.place_cursor(word.center());
    thread::sleep(Duration::from_millis(200));
    let mut pointer = Pointer::start(sway);
    // enter が GPUI に届いてからボタンを送る。先に送ると、面に入る前のボタンは捨てられる。
    thread::sleep(Duration::from_millis(200));
    pointer.click();
    thread::sleep(Duration::from_millis(400));
    word
}

fn wait_until_line(
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
        let text = flat(&recognize(shot_path));
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

fn write_tall(path: &str) {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  html, body { margin: 0; background: #ffffff; color: #111111; font-family: "Noto Sans", sans-serif; }
  h1 { font-size: 96px; line-height: 1.1; margin: 0; padding: 24px 32px; }
  .spacer { height: 1200px; }
</style>
</head>
<body>
  <h1 id="mark">susie-top</h1>
  <div class="spacer"></div>
  <h1>susie-low</h1>
  <div class="spacer"></div>
</body>
</html>
"#;
    fs::write(path, html).expect("write the tall page");
}

fn write_page(path: &str, heading: &str, field: bool) {
    let controls = if field {
        r#"<div class="controls"><button id="click" type="button">press</button><input id="field" type="text" spellcheck="false"></div>"#
    } else {
        ""
    };
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  html, body {{ margin: 0; background: #ffffff; color: #111111; font-family: "Noto Sans", sans-serif; }}
  h1 {{ font-size: 96px; line-height: 1.1; margin: 0; padding: 24px 32px; }}
  .controls {{ padding: 0 32px 32px; font-size: 24px; }}
  button, input {{ font: inherit; font-size: 24px; }}
  button {{ padding: 8px 16px; }}
  input {{ width: 420px; padding: 8px 16px; margin-left: 16px; font-size: 72px; }}
</style>
</head>
<body>
  <h1>{heading}</h1>
  {controls}
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

fn ends_of(video: &Path) -> (PathBuf, PathBuf) {
    let early = shot("s12-early.png");
    let late = shot("s12-late.png");
    extract_frame(video, &["-ss", "0.2"], &early);
    extract_frame(video, &["-sseof", "-0.3"], &late);
    (early, late)
}

fn shot(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}
