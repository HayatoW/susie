#![cfg(target_os = "linux")]

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use png::{BitDepth, ColorType, Decoder};
use serde_json::Value;

const LAVAPIPE_ICD: &str = "/usr/share/vulkan/icd.d/lvp_icd.json";
const EVIDENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/evidence/S02");
const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 800;
const STARTUP_DEADLINE: Duration = Duration::from_secs(45);
const EXIT_DEADLINE: Duration = Duration::from_secs(20);

const REPORT: &str = "\
ヘッドレスの Sway で、 susie を起動しました。
ウィンドウの中央の色は、画面の左上の色と違いました。
閉じる前、プロセスは動いていました。
ウィンドウへ閉じる要求を送りました。
プロセスは終了コード 0 で終わりました。
スクリーンショットは docs/evidence/S02/window.png です。
";

#[test]
#[ignore = "starts headless Sway and lavapipe, and writes docs/evidence/S02"]
fn closing_the_window_ends_the_process() {
    let sway = Sway::start();
    let mut app = App::spawn(&sway);
    let shot = wait_until_painted(&sway, &mut app);
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
    publish(&shot);
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
        let runtime = tmp.join("s02-xdg");
        let _ = fs::remove_dir_all(&runtime);
        fs::create_dir(&runtime).expect("create the Wayland runtime dir");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))
            .expect("restrict the Wayland runtime dir");

        let config = tmp.join("s02-sway.conf");
        fs::write(
            &config,
            format!(
                "output * bg #000000 solid_color\noutput HEADLESS-1 resolution {SCREEN_WIDTH}x{SCREEN_HEIGHT}\n"
            ),
        )
        .expect("write the Sway config");

        let stderr = tmp.join("s02-sway.stderr");
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

    fn window_rect(&self, pid: u32) -> Option<Rect> {
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
        let stderr = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s02-app.stderr");
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

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

fn wait_until_painted(sway: &Sway, app: &mut App) -> Vec<u8> {
    let deadline = Instant::now() + STARTUP_DEADLINE;
    let shot_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s02-window.png");
    let mut placed = false;
    let mut last = String::new();
    while Instant::now() < deadline {
        assert!(
            app.is_running(),
            "susie exited before a window was visible\n{}",
            app.stderr()
        );
        let Some(rect) = sway.window_rect(app.pid()) else {
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        if !placed {
            sway.place_window(app.pid());
            placed = true;
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        capture(sway, &shot_path);
        let shot = decode_png(&shot_path);
        assert_eq!(
            (shot.width, shot.height),
            (SCREEN_WIDTH, SCREEN_HEIGHT),
            "grim captured an unexpected screen"
        );
        if let Some(center) = content_center(&shot, rect) {
            let corner = rgb(&shot, 0, 0);
            let middle = rgb(&shot, center.0, center.1);
            if corner != middle {
                return fs::read(&shot_path).expect("read the screenshot");
            }
            last = format!("rect {rect:?} corner {corner:?} center {middle:?}");
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "the window center matched the screen corner for {}s\n{last}\n{}\n{}",
        STARTUP_DEADLINE.as_secs(),
        app.stderr(),
        read_log(&sway.stderr)
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

struct Shot {
    width: u32,
    height: u32,
    channels: usize,
    bytes: Vec<u8>,
}

fn decode_png(path: &Path) -> Shot {
    let file = File::open(path).expect("open the screenshot");
    let decoder = Decoder::new(BufReader::new(file));
    let mut reader = decoder.read_info().expect("read the png header");
    let mut bytes = vec![0; reader.output_buffer_size().expect("size the png")];
    let info = reader.next_frame(&mut bytes).expect("decode the png");
    let channels = match info.color_type {
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        other => panic!("unexpected screenshot color {other:?}"),
    };
    assert_eq!(info.bit_depth, BitDepth::Eight);
    bytes.truncate(info.buffer_size());
    Shot {
        width: info.width,
        height: info.height,
        channels,
        bytes,
    }
}

fn content_center(shot: &Shot, rect: Rect) -> Option<(u32, u32)> {
    if rect.x <= 0 || rect.y <= 0 || rect.width < 2 || rect.height < 2 {
        return None;
    }
    let x = rect.x + rect.width / 2;
    let y = rect.y + rect.height / 2;
    if x < 0 || y < 0 || x >= i64::from(shot.width) || y >= i64::from(shot.height) {
        return None;
    }
    Some((u32::try_from(x).ok()?, u32::try_from(y).ok()?))
}

fn rgb(shot: &Shot, x: u32, y: u32) -> [u8; 3] {
    let index = (y as usize * shot.width as usize + x as usize) * shot.channels;
    [
        shot.bytes[index],
        shot.bytes[index + 1],
        shot.bytes[index + 2],
    ]
}

fn find_window(value: &Value, pid: u32) -> Option<Rect> {
    if value.get("pid").and_then(Value::as_u64) == Some(u64::from(pid)) {
        let rect = value.get("rect")?;
        return Some(Rect {
            x: rect.get("x")?.as_i64()?,
            y: rect.get("y")?.as_i64()?,
            width: rect.get("width")?.as_i64()?,
            height: rect.get("height")?.as_i64()?,
        });
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = value.get(key).and_then(Value::as_array) {
            for child in children {
                if let Some(rect) = find_window(child, pid) {
                    return Some(rect);
                }
            }
        }
    }
    None
}

fn publish(png: &[u8]) {
    let dir = Path::new(EVIDENCE_DIR);
    fs::create_dir_all(dir).expect("create the evidence dir");
    write_atomic(&dir.join("window.png"), png);
    write_atomic(&dir.join("run.txt"), REPORT.as_bytes());
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
