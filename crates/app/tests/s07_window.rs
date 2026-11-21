#![cfg(target_os = "linux")]

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use png::{BitDepth, ColorType, Decoder};
use serde_json::Value;

const LAVAPIPE_ICD: &str = "/usr/share/vulkan/icd.d/lvp_icd.json";
const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S07");
const METRICS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/evidence/metrics.jsonl"
);
const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 800;
const STARTUP_DEADLINE: Duration = Duration::from_secs(120);
const TYPE_DEADLINE: Duration = Duration::from_secs(30);
const EXIT_DEADLINE: Duration = Duration::from_secs(20);
const MAX_VIDEO_BYTES: u64 = 10 * 1024 * 1024;

#[test]
#[ignore = "starts headless Sway, lavapipe, and CEF, and writes docs/evidence/S07"]
fn field_shows_susie() {
    let _metrics = MetricsFile::save();
    let sway = Sway::start();
    let mut app = App::spawn(&sway);
    let before = metric_lines();
    wait_until_top(&sway, &mut app, before);
    // 最初の configure が遅れて届くと、ウィンドウが出力いっぱいに戻る。
    // そのあとで置き直すと、クリックと打鍵のあいだサイズが保たれる。
    sway.place_window(app.pid());
    thread::sleep(Duration::from_millis(300));
    let mut recording = Recording::start(&sway);
    thread::sleep(Duration::from_millis(500));
    type_until_shown(&sway, &mut app);
    thread::sleep(Duration::from_millis(700));
    let raw = recording.stop();
    let video = transcode(&raw);
    assert_playable(&video);
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
    publish(&video);
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

struct MetricsFile {
    original: Vec<u8>,
    existed: bool,
}

impl MetricsFile {
    fn save() -> Self {
        let path = Path::new(METRICS_PATH);
        if path.exists() {
            Self {
                original: fs::read(path).expect("read metrics before the run"),
                existed: true,
            }
        } else {
            Self {
                original: Vec::new(),
                existed: false,
            }
        }
    }
}

impl Drop for MetricsFile {
    fn drop(&mut self) {
        let path = Path::new(METRICS_PATH);
        if self.existed {
            fs::write(path, &self.original).expect("restore metrics");
        } else if path.exists() {
            fs::remove_file(path).expect("remove metrics created by this run");
        }
    }
}

struct Sway {
    child: Child,
    runtime: PathBuf,
    socket: PathBuf,
    display: String,
    stderr: PathBuf,
}

impl Sway {
    fn start() -> Self {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let runtime = tmp.join("s07-xdg");
        let _ = fs::remove_dir_all(&runtime);
        fs::create_dir(&runtime).expect("create the Wayland runtime dir");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))
            .expect("restrict the Wayland runtime dir");

        let config = tmp.join("s07-sway.conf");
        fs::write(
            &config,
            format!(
                "output * bg #000000 solid_color\noutput HEADLESS-1 resolution {SCREEN_WIDTH}x{SCREEN_HEIGHT}\n"
            ),
        )
        .expect("write the Sway config");

        let stderr = tmp.join("s07-sway.stderr");
        let child = Command::new("sway")
            .arg("-c")
            .arg(&config)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("WLR_BACKENDS", "headless")
            .env("WLR_RENDERER", "pixman")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .env("WLR_HEADLESS_OUTPUTS", "1")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("SWAYSOCK")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(File::create(&stderr).expect("create the Sway stderr log"))
            .spawn()
            .expect("spawn Sway");

        let pid = child.id();
        let mut session = Self {
            child,
            runtime,
            socket: PathBuf::new(),
            display: String::new(),
            stderr,
        };
        session.socket = session.wait_for_file(&format!(".{pid}.sock"));
        session.display = session
            .wait_for_file("wayland-")
            .file_name()
            .expect("the Wayland socket has a name")
            .to_string_lossy()
            .into_owned();
        session
    }

    fn wait_for_file(&mut self, marker: &str) -> PathBuf {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(path) = fs::read_dir(&self.runtime)
                .expect("read the Wayland runtime dir")
                .flatten()
                .map(|entry| entry.path())
                .find(|path| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    name.contains(marker) && !name.ends_with(".lock")
                })
            {
                return path;
            }
            assert!(
                self.child.try_wait().expect("poll Sway").is_none(),
                "Sway exited before it was ready\n{}",
                read_log(&self.stderr)
            );
            assert!(
                Instant::now() < deadline,
                "Sway did not create {marker}\n{}",
                read_log(&self.stderr)
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn close_window(&self, pid: u32) {
        self.swaymsg(&format!("[pid={pid}] kill"));
    }

    fn focus_window(&self, pid: u32) {
        self.swaymsg(&format!("[pid={pid}] focus"));
    }

    fn has_window(&self, pid: u32) -> bool {
        let tree: Value =
            serde_json::from_str(&self.swaymsg("-t get_tree")).expect("parse the Sway tree");
        find_window(&tree, pid)
    }

    fn swaymsg(&self, args: &str) -> String {
        let output = Command::new("swaymsg")
            .arg("-s")
            .arg(&self.socket)
            .args(args.split_whitespace())
            .output()
            .expect("run swaymsg");
        assert!(
            output.status.success(),
            "swaymsg {args} failed\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("swaymsg stdout is utf-8")
    }

    fn place_window(&self, pid: u32) {
        self.swaymsg(&format!(
            "[pid={pid}] floating enable, resize set 800 600, move position center"
        ));
    }
}

impl Drop for Sway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct App {
    child: Child,
    stderr: PathBuf,
}

impl App {
    fn spawn(sway: &Sway) -> Self {
        assert!(
            Path::new(LAVAPIPE_ICD).is_file(),
            "lavapipe ICD is missing at {LAVAPIPE_ICD}"
        );
        let stderr = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s07-app.stderr");
        let child = Command::new(env!("CARGO_BIN_EXE_susie"))
            .env("XDG_RUNTIME_DIR", &sway.runtime)
            .env("WAYLAND_DISPLAY", &sway.display)
            .env("VK_ICD_FILENAMES", LAVAPIPE_ICD)
            .env_remove("DISPLAY")
            .env_remove("ZED_HEADLESS")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(File::create(&stderr).expect("create the susie stderr log"))
            .spawn()
            .expect("spawn susie");
        Self { child, stderr }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn is_running(&mut self) -> bool {
        self.child.try_wait().expect("poll susie").is_none()
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + EXIT_DEADLINE;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll susie") {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "susie was still running {}s after the close request\n{}",
                EXIT_DEADLINE.as_secs(),
                self.stderr()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn stderr(&self) -> String {
        read_log(&self.stderr)
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct Recording {
    child: Child,
    raw: PathBuf,
    stderr: PathBuf,
}

impl Recording {
    fn start(sway: &Sway) -> Self {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let raw = tmp.join("s07-raw.mp4");
        let stderr = tmp.join("s07-record.stderr");
        let _ = fs::remove_file(&raw);
        let child = Command::new("wf-recorder")
            .args([
                "-f",
                raw.to_str().expect("raw path is utf-8"),
                "-c",
                "libx264",
                "-x",
                "yuv420p",
                "-r",
                "15",
                "-p",
                "crf=28",
                "-p",
                "preset=veryfast",
            ])
            .env("XDG_RUNTIME_DIR", &sway.runtime)
            .env("WAYLAND_DISPLAY", &sway.display)
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(File::create(&stderr).expect("create the recorder stderr log"))
            .spawn()
            .expect("spawn wf-recorder");
        Self { child, raw, stderr }
    }

    fn stop(&mut self) -> PathBuf {
        let _ = Command::new("kill")
            .args(["-INT", &self.child.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().expect("poll wf-recorder") {
                assert!(
                    status.success(),
                    "wf-recorder failed: {status}\n{}",
                    read_log(&self.stderr)
                );
                return self.raw.clone();
            }
            assert!(
                Instant::now() < deadline,
                "wf-recorder did not finish\n{}",
                read_log(&self.stderr)
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("kill")
                .args(["-INT", &self.child.id().to_string()])
                .status();
            let _ = self.child.wait();
        }
    }
}

fn wait_until_top(sway: &Sway, app: &mut App, metrics_before: usize) {
    let deadline = Instant::now() + STARTUP_DEADLINE;
    let shot_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s07-top.png");
    let mut placed = false;
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited before a window was visible\n{}",
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
        capture(sway, &shot_path);
        let text = recognize(&shot_path);
        let shows_top = text.contains("susie-top");
        let shows_press = text.contains("press");
        let shows_clicked = text.contains("susie-clicked");
        let lines = metric_lines();
        if shows_top && shows_press && !shows_clicked && lines == metrics_before + 1 {
            assert_metric(&lines_of_metrics()[metrics_before]);
            return;
        }
        last = format!(
            "top {shows_top} press {shows_press} clicked {shows_clicked} metrics {lines} text {text:?}"
        );
        thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "the button was not shown before the click within {}s\n{last}\n{}\n{}",
        STARTUP_DEADLINE.as_secs(),
        app.stderr(),
        read_log(&sway.stderr)
    );
}

fn type_until_shown(sway: &Sway, app: &mut App) {
    let deadline = Instant::now() + TYPE_DEADLINE;
    let shot_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s07-field.png");
    let press = press_word(sway, &shot_path);
    place_pointer(sway, field_spot(&press));
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

#[derive(Clone, Copy, Debug)]
struct Spot {
    x: i32,
    y: i32,
}

fn press_word(sway: &Sway, shot: &Path) -> Word {
    capture(sway, shot);
    find_word(shot, "press")
        .unwrap_or_else(|| panic!("press was not on screen\n{}", recognize(shot)))
}

fn field_spot(press: &Word) -> Spot {
    Spot {
        x: press.left + press.width + 90,
        y: press.top + press.height / 2,
    }
}

fn field_has(path: &Path, text: &str) -> bool {
    // 欄の文字は一塊として読む。見出しの susie-top は上にあり、欄の語はそれより下。
    let found = words(path, "6");
    let Some(anchor) = found.iter().map(|word| word.top).min() else {
        return false;
    };
    found
        .iter()
        .any(|word| word.text == text && word.top >= anchor + 60)
}

struct Word {
    text: String,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

fn find_word(path: &Path, word: &str) -> Option<Word> {
    words(path, "11")
        .into_iter()
        .find(|found| found.text == word)
}

fn words(path: &Path, psm: &str) -> Vec<Word> {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("--psm")
        .arg(psm)
        .arg("tsv")
        .output()
        .expect("run tesseract tsv");
    assert!(
        output.status.success(),
        "tesseract tsv failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let mut found = Vec::new();
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let Ok(left) = cols[6].parse::<i32>() else {
            continue;
        };
        let Ok(top) = cols[7].parse::<i32>() else {
            continue;
        };
        let Ok(width) = cols[8].parse::<i32>() else {
            continue;
        };
        let Ok(height) = cols[9].parse::<i32>() else {
            continue;
        };
        let word = cols[11].trim().to_lowercase();
        if word.is_empty() {
            continue;
        }
        found.push(Word {
            text: word,
            left,
            top,
            width,
            height,
        });
    }
    found
}

fn place_pointer(sway: &Sway, spot: Spot) {
    sway.swaymsg(&format!("seat seat0 cursor set {} {}", spot.x, spot.y));
}

struct Pointer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Pointer {
    fn start(sway: &Sway) -> Self {
        let bin = compile_pointer_click();
        let mut child = Command::new(&bin)
            .arg("hold")
            .env("XDG_RUNTIME_DIR", &sway.runtime)
            .env("WAYLAND_DISPLAY", &sway.display)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DEBUG")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the virtual pointer");
        let stdin = child.stdin.take().expect("pointer stdin");
        let stdout = BufReader::new(child.stdout.take().expect("pointer stdout"));
        let mut pointer = Self {
            child,
            stdin,
            stdout,
        };
        pointer.read_line("ready");
        pointer
    }

    fn click(&mut self) {
        writeln!(self.stdin, "click").expect("write a click");
        self.stdin.flush().expect("flush a click");
        self.read_line("ok");
    }

    fn read_line(&mut self, expected: &str) {
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("read the virtual pointer");
        assert_eq!(line.trim(), expected, "virtual pointer replied {line:?}");
    }
}

impl Drop for Pointer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn compile_pointer_click() -> PathBuf {
    let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/pointer");
    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s07-pointer");
    fs::create_dir_all(&out_dir).expect("create the pointer build dir");
    let xml = src_dir.join("wlr-virtual-pointer-unstable-v1.xml");
    let source = src_dir.join("pointer_click.c");
    let header = out_dir.join("wlr-virtual-pointer-unstable-v1-client-protocol.h");
    let protocol = out_dir.join("wlr-virtual-pointer-unstable-v1-protocol.c");
    let bin = out_dir.join("pointer_click");
    run_tool(
        "wayland-scanner",
        &[
            "client-header",
            xml.to_str().expect("xml path is utf-8"),
            header.to_str().expect("header path is utf-8"),
        ],
    );
    run_tool(
        "wayland-scanner",
        &[
            "private-code",
            xml.to_str().expect("xml path is utf-8"),
            protocol.to_str().expect("protocol path is utf-8"),
        ],
    );
    run_tool(
        "gcc",
        &[
            "-O2",
            "-I",
            out_dir.to_str().expect("out dir is utf-8"),
            "-o",
            bin.to_str().expect("bin path is utf-8"),
            source.to_str().expect("source path is utf-8"),
            protocol.to_str().expect("protocol path is utf-8"),
            "-lwayland-client",
        ],
    );
    bin
}

struct Keyboard {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Keyboard {
    fn start(sway: &Sway) -> Self {
        let bin = compile_keyboard_type();
        let mut child = Command::new(bin)
            .arg("hold")
            .env("XDG_RUNTIME_DIR", &sway.runtime)
            .env("WAYLAND_DISPLAY", &sway.display)
            .env_remove("DISPLAY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the virtual keyboard");
        let stdin = child.stdin.take().expect("keyboard stdin");
        let stdout = BufReader::new(child.stdout.take().expect("keyboard stdout"));
        let mut keyboard = Self {
            child,
            stdin,
            stdout,
        };
        keyboard.read_line("ready");
        keyboard
    }

    fn type_text(&mut self, text: &str) {
        writeln!(self.stdin, "{text}").expect("write the text");
        self.stdin.flush().expect("flush the text");
        self.read_line("ok");
    }

    fn read_line(&mut self, expected: &str) {
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line);
        if line.trim() == expected {
            return;
        }
        let _ = self.child.kill();
        let mut err = String::new();
        if let Some(mut stderr) = self.child.stderr.take() {
            let _ = stderr.read_to_string(&mut err);
        }
        panic!("virtual keyboard replied {line:?} (expected {expected}, read {read:?})\n{err}");
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn compile_keyboard_type() -> PathBuf {
    let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/keyboard");
    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s07-keyboard");
    fs::create_dir_all(&out_dir).expect("create the keyboard build dir");
    let xml = src_dir.join("virtual-keyboard-unstable-v1.xml");
    let source = src_dir.join("keyboard_type.c");
    let header = out_dir.join("virtual-keyboard-unstable-v1-client-protocol.h");
    let protocol = out_dir.join("virtual-keyboard-unstable-v1-protocol.c");
    let bin = out_dir.join("keyboard_type");
    run_tool(
        "wayland-scanner",
        &[
            "client-header",
            xml.to_str().expect("xml path is utf-8"),
            header.to_str().expect("header path is utf-8"),
        ],
    );
    run_tool(
        "wayland-scanner",
        &[
            "private-code",
            xml.to_str().expect("xml path is utf-8"),
            protocol.to_str().expect("protocol path is utf-8"),
        ],
    );
    run_tool(
        "gcc",
        &[
            "-O2",
            "-I",
            out_dir.to_str().expect("out dir is utf-8"),
            "-o",
            bin.to_str().expect("bin path is utf-8"),
            source.to_str().expect("source path is utf-8"),
            protocol.to_str().expect("protocol path is utf-8"),
            "-lwayland-client",
            "-lxkbcommon",
        ],
    );
    bin
}

fn run_tool(program: &str, args: &[&str]) {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("run {program}: {err}"));
    assert!(
        output.status.success(),
        "{program} failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn capture(sway: &Sway, dest: &Path) {
    let output = Command::new("grim")
        .env("XDG_RUNTIME_DIR", &sway.runtime)
        .env("WAYLAND_DISPLAY", &sway.display)
        .env_remove("DISPLAY")
        .arg(dest)
        .output()
        .expect("run grim");
    assert!(
        output.status.success(),
        "grim failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn recognize(path: &Path) -> String {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("--psm")
        .arg("11")
        .output()
        .expect("run tesseract");
    assert!(
        output.status.success(),
        "tesseract failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .to_lowercase()
        .to_string()
}

fn transcode(raw: &Path) -> PathBuf {
    let dest = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s07-type.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            raw.to_str().expect("raw path is utf-8"),
            "-an",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-profile:v",
            "high",
            "-level",
            "4.0",
            "-movflags",
            "+faststart",
            "-crf",
            "28",
            "-preset",
            "veryfast",
            dest.to_str().expect("video path is utf-8"),
        ])
        .output()
        .expect("run ffmpeg");
    assert!(
        output.status.success(),
        "ffmpeg failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    dest
}

fn assert_playable(path: &Path) {
    let bytes = fs::read(path).expect("read the recording");
    let len = u64::try_from(bytes.len()).expect("recording size fits in u64");
    assert!(
        len < MAX_VIDEO_BYTES,
        "recording is {len} bytes, GitHub cannot show a file this large inline"
    );
    assert!(
        moov_before_mdat(&bytes),
        "moov is not before mdat, so GitHub cannot start playback"
    );
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            path.to_str().expect("video path is utf-8"),
        ])
        .output()
        .expect("run ffprobe");
    assert!(
        output.status.success(),
        "ffprobe failed\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("ffprobe json");
    let streams = value
        .get("streams")
        .and_then(Value::as_array)
        .expect("ffprobe streams");
    assert_eq!(streams.len(), 1, "the recording should be video only");
    let stream = &streams[0];
    assert_eq!(
        stream.get("codec_name").and_then(Value::as_str),
        Some("h264")
    );
    assert_eq!(
        stream.get("pix_fmt").and_then(Value::as_str),
        Some("yuv420p")
    );
    let width = stream.get("width").and_then(Value::as_u64).unwrap_or(0);
    let height = stream.get("height").and_then(Value::as_u64).unwrap_or(0);
    assert_eq!(
        (width, height),
        (u64::from(SCREEN_WIDTH), u64::from(SCREEN_HEIGHT))
    );
    let duration = value
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(0.0);
    assert!(
        duration >= 1.0,
        "recording is too short to show the click: {duration}"
    );
}

fn ends_of(video: &Path) -> (PathBuf, PathBuf) {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let early = tmp.join("s07-early.png");
    let late = tmp.join("s07-late.png");
    extract_frame(video, &["-ss", "0.2"], &early);
    extract_frame(video, &["-sseof", "-0.3"], &late);
    (early, late)
}

fn extract_frame(video: &Path, seek: &[&str], dest: &Path) {
    let mut args = vec!["-y".to_string()];
    args.extend(seek.iter().map(|arg| (*arg).to_string()));
    args.push("-i".to_string());
    args.push(video.to_str().expect("video path is utf-8").to_string());
    args.extend(["-frames:v".to_string(), "1".to_string()]);
    args.push(dest.to_str().expect("frame path is utf-8").to_string());
    let output = Command::new("ffmpeg")
        .args(&args)
        .output()
        .expect("extract a frame");
    assert!(
        output.status.success(),
        "ffmpeg could not extract a frame\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let decoded = decode_png(dest);
    assert_eq!(
        (decoded.width, decoded.height),
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    );
}

fn moov_before_mdat(bytes: &[u8]) -> bool {
    let mut offset = 0usize;
    let mut saw_moov = false;
    while offset + 8 <= bytes.len() {
        let size32 = u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("box size"));
        let kind = &bytes[offset + 4..offset + 8];
        let size = if size32 == 1 {
            if offset + 16 > bytes.len() {
                return false;
            }
            let wide = u64::from_be_bytes(
                bytes[offset + 8..offset + 16]
                    .try_into()
                    .expect("wide size"),
            );
            usize::try_from(wide).unwrap_or(usize::MAX)
        } else if size32 == 0 {
            bytes.len() - offset
        } else {
            size32 as usize
        };
        if kind == b"moov" {
            saw_moov = true;
        }
        if kind == b"mdat" {
            return saw_moov;
        }
        if size < 8 || offset.saturating_add(size) > bytes.len() {
            return false;
        }
        offset += size;
    }
    false
}

fn metric_lines() -> usize {
    lines_of_metrics().len()
}

fn lines_of_metrics() -> Vec<String> {
    let path = Path::new(METRICS_PATH);
    if !path.exists() {
        return Vec::new();
    }
    fs::read_to_string(path)
        .expect("read metrics")
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn assert_metric(line: &str) {
    let value: Value = serde_json::from_str(line).expect("metrics line is json");
    let startup_ms = value.get("startup_ms").and_then(Value::as_u64);
    let fps = value.get("fps").and_then(Value::as_u64);
    assert!(startup_ms.is_some(), "startup_ms is missing from {line}");
    assert!(fps.is_some(), "fps is missing from {line}");
    assert!(
        fps.is_some_and(|fps| fps >= 1),
        "fps did not count a frame: {line}"
    );
}

struct Shot {
    width: u32,
    height: u32,
}

fn decode_png(path: &Path) -> Shot {
    let file = File::open(path).expect("open the frame");
    let decoder = Decoder::new(BufReader::new(file));
    let mut reader = decoder.read_info().expect("read the png header");
    let mut bytes = vec![0; reader.output_buffer_size().expect("size the png")];
    let info = reader.next_frame(&mut bytes).expect("decode the png");
    match info.color_type {
        ColorType::Rgb | ColorType::Rgba => {}
        other => panic!("unexpected frame color {other:?}"),
    }
    assert_eq!(info.bit_depth, BitDepth::Eight);
    Shot {
        width: info.width,
        height: info.height,
    }
}

fn find_window(value: &Value, pid: u32) -> bool {
    if value.get("pid").and_then(Value::as_u64) == Some(u64::from(pid)) {
        return true;
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = value.get(key).and_then(Value::as_array) {
            for child in children {
                if find_window(child, pid) {
                    return true;
                }
            }
        }
    }
    false
}

fn publish(video: &Path) {
    let dir = Path::new(EVIDENCE_DIR);
    fs::create_dir_all(dir).expect("create the evidence dir");
    let bytes = fs::read(video).expect("read the recording to publish");
    write_atomic(&dir.join("type.mp4"), &bytes);
}

fn write_atomic(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("partial");
    fs::write(&tmp, bytes).expect("write a temporary evidence file");
    fs::rename(&tmp, path).expect("publish an evidence file");
}

fn read_log(path: &Path) -> String {
    let mut file = File::open(path).unwrap_or_else(|_| panic!("open {}", path.display()));
    let mut text = String::new();
    file.read_to_string(&mut text)
        .unwrap_or_else(|_| panic!("read {}", path.display()));
    text
}
