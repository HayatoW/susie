#![cfg(target_os = "linux")]

mod harness;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness::{App, MetricsFile, Sway, capture, publish, recognize_block};

// example.com は IANA が文書例として維持するページで、見出しは Example Domain のまま変わらない。
const EXAMPLE_URL: &str = "https://example.com";
const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S09");
const PAGE_DEADLINE: Duration = Duration::from_secs(120);

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S09"]
fn argument_url_is_visible_and_no_argument_opens_the_fixture() {
    let _metrics = MetricsFile::save();
    let sway = Sway::start("s09");

    let mut app = App::spawn_with(&sway, &[EXAMPLE_URL]);
    let site = shot("s09-example.png");
    wait_until_page(&sway, &mut app, &site, "example domain", "susie-top");
    close_cleanly(&sway, &mut app);

    let mut app = App::spawn(&sway);
    let fixture = shot("s09-fixture.png");
    wait_until_page(&sway, &mut app, &fixture, "susie-top", "example domain");
    close_cleanly(&sway, &mut app);

    publish(&site, &Path::new(EVIDENCE_DIR).join("window.png"));
}

fn wait_until_page(sway: &Sway, app: &mut App, shot_path: &Path, present: &str, absent: &str) {
    let deadline = Instant::now() + PAGE_DEADLINE;
    let mut placed = false;
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited before {present} was visible\n{}",
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
        if text.contains(present) && !text.contains(absent) {
            return;
        }
        last = text;
        thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "{present} was not visible within {}s\n{last}\n{}\n{}",
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
