use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use png::{BitDepth, ColorType, Decoder};
use serde_json::Value;

use super::session::{self, App, SCREEN_HEIGHT, SCREEN_WIDTH, STARTUP_DEADLINE, Spot, Sway};

const MAX_VIDEO_BYTES: u64 = 10 * 1024 * 1024;

pub struct Word {
    pub text: String,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl Word {
    pub fn center(&self) -> Spot {
        Spot {
            x: self.left + self.width / 2,
            y: self.top + self.height / 2,
        }
    }
}

pub struct Recording {
    child: Child,
    raw: PathBuf,
    stderr_path: PathBuf,
}

impl Recording {
    pub fn start(sway: &Sway) -> Self {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let raw = tmp.join(format!("{}-raw.mp4", sway.name()));
        let stderr_path = tmp.join(format!("{}-record.stderr", sway.name()));
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
            .env("XDG_RUNTIME_DIR", sway.runtime())
            .env("WAYLAND_DISPLAY", sway.display())
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(File::create(&stderr_path).expect("create the recorder stderr log"))
            .spawn()
            .expect("spawn wf-recorder");
        Self {
            child,
            raw,
            stderr_path,
        }
    }

    pub fn stop(&mut self) -> PathBuf {
        let _ = Command::new("kill")
            .args(["-INT", &self.child.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().expect("poll wf-recorder") {
                assert!(
                    status.success(),
                    "wf-recorder failed: {status}\n{}",
                    session::read_log(&self.stderr_path)
                );
                return self.raw.clone();
            }
            assert!(
                Instant::now() < deadline,
                "wf-recorder did not finish\n{}",
                session::read_log(&self.stderr_path)
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

pub fn wait_until_ready(
    sway: &Sway,
    app: &mut App,
    metrics_before: usize,
    shot: &Path,
    what: &str,
) {
    let deadline = Instant::now() + STARTUP_DEADLINE;
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
        session::capture(sway, shot);
        let text = recognize(shot);
        let shows_top = text.contains("susie-top");
        let shows_press = text.contains("press");
        let shows_clicked = text.contains("susie-clicked");
        let lines = session::metric_lines();
        if shows_top && shows_press && !shows_clicked && lines == metrics_before + 1 {
            session::assert_metric(&session::lines_of_metrics()[metrics_before]);
            return;
        }
        last = format!(
            "top {shows_top} press {shows_press} clicked {shows_clicked} metrics {lines} text {text:?}"
        );
        thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "the button was not shown before the {what} within {}s\n{last}\n{}\n{}",
        STARTUP_DEADLINE.as_secs(),
        app.stderr(),
        sway.stderr()
    );
}

pub fn require_word(sway: &Sway, shot: &Path, word: &str) -> Word {
    session::capture(sway, shot);
    find_word(shot, word).unwrap_or_else(|| panic!("{word} was not on screen\n{}", recognize(shot)))
}

pub fn field_spot(press: &Word) -> Spot {
    Spot {
        x: press.left + press.width + 90,
        y: press.top + press.height / 2,
    }
}

pub fn field_has(path: &Path, text: &str) -> bool {
    // 欄の文字は一塊として読む。見出しの susie-top は上にあり、欄の語はそれより下。
    let found = words(path, "6");
    let Some(anchor) = found.iter().map(|word| word.top).min() else {
        return false;
    };
    found
        .iter()
        .any(|word| word.text == text && word.top >= anchor + 60)
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

pub fn recognize(path: &Path) -> String {
    recognize_with(path, "11")
}

pub fn recognize_block(path: &Path) -> String {
    recognize_with(path, "6")
}

fn recognize_with(path: &Path, psm: &str) -> String {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("--psm")
        .arg(psm)
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

pub fn transcode(raw: &Path, dest: &Path) {
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
}

pub fn assert_playable(path: &Path, min_secs: f64, what: &str) {
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
    let value = probe(path);
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
    let duration = duration_of(&value);
    assert!(
        duration >= min_secs,
        "recording is too short to show the {what}: {duration}"
    );
}

pub fn duration_secs(path: &Path) -> f64 {
    duration_of(&probe(path))
}

fn probe(path: &Path) -> Value {
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
    serde_json::from_slice(&output.stdout).expect("ffprobe json")
}

fn duration_of(value: &Value) -> f64 {
    value
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(0.0)
}

pub fn extract_frame(video: &Path, seek: &[&str], dest: &Path) {
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
    assert_eq!(decoded, (SCREEN_WIDTH, SCREEN_HEIGHT));
}

fn decode_png(path: &Path) -> (u32, u32) {
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
    (info.width, info.height)
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

pub fn publish(video: &Path, dest: &Path) {
    if let Some(dir) = dest.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir).expect("create the evidence dir");
    }
    let bytes = fs::read(video).expect("read the recording to publish");
    write_atomic(dest, &bytes);
}

fn write_atomic(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("partial");
    fs::write(&tmp, bytes).expect("write a temporary evidence file");
    fs::rename(&tmp, path).expect("publish an evidence file");
}
