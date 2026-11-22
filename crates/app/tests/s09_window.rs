#![cfg(target_os = "linux")]

mod harness;

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness::{App, MetricsFile, Sway, capture, publish, recognize_block};

// RFC 2606 は発行後に本文が変わらない。先頭に番号と題名が出る。
const PAGE_URL: &str = "https://www.rfc-editor.org/rfc/rfc2606.txt";
const PAGE_MARK: &str = "reserved top level dns names";
const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S09");
const PAGE_DEADLINE: Duration = Duration::from_secs(120);

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S09"]
fn argument_url_is_visible_and_no_argument_opens_the_fixture() {
    let _metrics = MetricsFile::save();
    let sway = Sway::start("s09");

    let mut app = App::spawn_with(&sway, &[PAGE_URL]);
    let site = shot("s09-rfc.png");
    wait_until_page(&sway, &mut app, &site, &["2606", PAGE_MARK], "susie-top");
    close_cleanly(&sway, &mut app);

    let mut app = App::spawn(&sway);
    let fixture = shot("s09-fixture.png");
    wait_until_page(&sway, &mut app, &fixture, &["susie-top"], PAGE_MARK);
    close_cleanly(&sway, &mut app);

    // 上端 200px は白い。見出しはその下で、表示範囲の中にある。
    let lower = file_page("s09-lower.html", LOWER_PAGE);
    let mut app = App::spawn_with(&sway, &[&lower]);
    let lower_shot = shot("s09-lower.png");
    wait_until_page(&sway, &mut app, &lower_shot, &["susie-lower"], "susie-top");
    close_cleanly(&sway, &mut app);

    publish(&site, &Path::new(EVIDENCE_DIR).join("window.png"));
}

const LOWER_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  html, body { margin: 0; background: #ffffff; color: #111111; font-family: "Noto Sans", sans-serif; }
  .spacer { height: 200px; }
  h1 { font-size: 72px; line-height: 1.1; margin: 0; padding: 32px; }
</style>
</head>
<body>
  <div class="spacer"></div>
  <h1>susie-lower</h1>
</body>
</html>
"#;

fn file_page(name: &str, html: &str) -> String {
    let path = shot(name);
    fs::write(&path, html).expect("write the page");
    let text = path.to_str().expect("page path is utf-8");
    format!("file://{text}")
}

fn wait_until_page(sway: &Sway, app: &mut App, shot_path: &Path, present: &[&str], absent: &str) {
    let deadline = Instant::now() + PAGE_DEADLINE;
    let mut placed = false;
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited before {present:?} was visible\n{}",
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
        if present.iter().all(|phrase| text.contains(phrase)) && !text.contains(absent) {
            return;
        }
        last = text;
        thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "{present:?} was not visible within {}s\n{last}\n{}\n{}",
        PAGE_DEADLINE.as_secs(),
        app.stderr(),
        sway.stderr()
    );
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
