use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

pub const SCREEN_WIDTH: u32 = 1280;
pub const SCREEN_HEIGHT: u32 = 800;
pub(crate) const STARTUP_DEADLINE: Duration = Duration::from_secs(120);
const EXIT_DEADLINE: Duration = Duration::from_secs(20);
const METRICS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/evidence/metrics.jsonl"
);
const LAVAPIPE_ICD: &str = "/usr/share/vulkan/icd.d/lvp_icd.json";

#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub x: i32,
    pub y: i32,
}

pub struct MetricsFile {
    original: Vec<u8>,
    existed: bool,
    // 残すときだけ、この起動が足した行を入れる。
    kept: Option<String>,
}

impl MetricsFile {
    pub fn save() -> Self {
        let path = Path::new(METRICS_PATH);
        if path.exists() {
            Self {
                original: fs::read(path).expect("read metrics before the run"),
                existed: true,
                kept: None,
            }
        } else {
            Self {
                original: Vec::new(),
                existed: false,
                kept: None,
            }
        }
    }

    // 起動が足した行を残す。呼ばなければ、 Drop が計測ファイルを起動前へ戻す。
    // 最初の行は startup_ms と fps。ページの左クリックを渡した起動は、そのあとに input_ms が 1 行ある。
    pub fn keep_new_line(&mut self) {
        let lines = lines_of_metrics();
        let original_count = String::from_utf8_lossy(&self.original)
            .lines()
            .filter(|line| !line.is_empty())
            .count();
        let added = lines.len().saturating_sub(original_count);
        assert!(
            added == 1 || added == 2,
            "this launch should append startup metrics, and input_ms only after a page press: {lines:?}"
        );
        assert_metric(&lines[original_count]);
        if added == 2 {
            assert_input(&lines[original_count + 1]);
        }
        self.kept = Some(lines[original_count..].join("\n"));
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
        let Some(line) = self.kept.as_deref() else {
            return;
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("append the kept metrics line");
        writeln!(file, "{line}").expect("write the kept metrics line");
    }
}

pub struct Sway {
    child: Child,
    name: String,
    runtime: PathBuf,
    socket: PathBuf,
    display: String,
    stderr_path: PathBuf,
}

impl Sway {
    pub fn start(name: &str) -> Self {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let runtime = tmp.join(format!("{name}-xdg"));
        let _ = fs::remove_dir_all(&runtime);
        fs::create_dir(&runtime).expect("create the Wayland runtime dir");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))
            .expect("restrict the Wayland runtime dir");

        let config = tmp.join(format!("{name}-sway.conf"));
        fs::write(
            &config,
            format!(
                "output * bg #000000 solid_color\noutput HEADLESS-1 resolution {SCREEN_WIDTH}x{SCREEN_HEIGHT}\n"
            ),
        )
        .expect("write the Sway config");

        let stderr_path = tmp.join(format!("{name}-sway.stderr"));
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
            .stderr(File::create(&stderr_path).expect("create the Sway stderr log"))
            .spawn()
            .expect("spawn Sway");

        let pid = child.id();
        let mut session = Self {
            child,
            name: name.to_string(),
            runtime,
            socket: PathBuf::new(),
            display: String::new(),
            stderr_path,
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

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn runtime(&self) -> &Path {
        &self.runtime
    }

    pub(crate) fn display(&self) -> &str {
        &self.display
    }

    pub fn stderr(&self) -> String {
        read_log(&self.stderr_path)
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
                self.stderr()
            );
            assert!(
                Instant::now() < deadline,
                "Sway did not create {marker}\n{}",
                self.stderr()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn close_window(&self, pid: u32) {
        self.swaymsg(&format!("[pid={pid}] kill"));
    }

    pub fn focus_window(&self, pid: u32) {
        self.swaymsg(&format!("[pid={pid}] focus"));
    }

    pub fn has_window(&self, pid: u32) -> bool {
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

    pub fn place_window(&self, pid: u32) {
        self.swaymsg(&format!(
            "[pid={pid}] floating enable, resize set 800 600, move position center"
        ));
    }

    pub fn place_cursor(&self, spot: Spot) {
        self.swaymsg(&format!("seat seat0 cursor set {} {}", spot.x, spot.y));
    }
}

impl Drop for Sway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct App {
    child: Child,
    stderr_path: PathBuf,
}

impl App {
    pub fn spawn(sway: &Sway) -> Self {
        Self::spawn_with(sway, &[])
    }

    pub fn spawn_with(sway: &Sway, args: &[&str]) -> Self {
        assert!(
            Path::new(LAVAPIPE_ICD).is_file(),
            "lavapipe ICD is missing at {LAVAPIPE_ICD}"
        );
        let stderr_path =
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{}-app.stderr", sway.name));
        let child = Command::new(env!("CARGO_BIN_EXE_susie"))
            .args(args)
            .env("XDG_RUNTIME_DIR", &sway.runtime)
            .env("WAYLAND_DISPLAY", &sway.display)
            .env("VK_ICD_FILENAMES", LAVAPIPE_ICD)
            .env_remove("DISPLAY")
            .env_remove("ZED_HEADLESS")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(File::create(&stderr_path).expect("create the susie stderr log"))
            .spawn()
            .expect("spawn susie");
        Self { child, stderr_path }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().expect("poll susie").is_none()
    }

    pub fn wait_for_exit(&mut self) -> ExitStatus {
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

    pub fn stderr(&self) -> String {
        read_log(&self.stderr_path)
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

pub fn capture(sway: &Sway, dest: &Path) {
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

pub fn metric_lines() -> usize {
    lines_of_metrics().len()
}

/// `before` より前の行は、前の起動の計測なので見ない。
pub fn input_ms_since(before: usize) -> Option<u64> {
    for line in lines_of_metrics().into_iter().skip(before) {
        let value: Value = serde_json::from_str(&line).expect("metrics line is json");
        if let Some(input_ms) = value.get("input_ms").and_then(Value::as_u64) {
            return Some(input_ms);
        }
    }
    None
}

pub(crate) fn lines_of_metrics() -> Vec<String> {
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

pub(crate) fn assert_metric(line: &str) {
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

pub(crate) fn assert_input(line: &str) {
    let value: Value = serde_json::from_str(line).expect("input line is json");
    assert!(
        value.get("input_ms").and_then(Value::as_u64).is_some(),
        "input_ms is missing from {line}"
    );
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

pub(crate) fn read_log(path: &Path) -> String {
    let mut file = File::open(path).unwrap_or_else(|_| panic!("open {}", path.display()));
    let mut text = String::new();
    file.read_to_string(&mut text)
        .unwrap_or_else(|_| panic!("read {}", path.display()));
    text
}
