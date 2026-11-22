use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use super::session::Sway;

pub struct Pointer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Pointer {
    pub fn start(sway: &Sway) -> Self {
        let bin = compile_click(sway.name());
        let (child, stdin, stdout) = spawn_held(&bin, sway);
        let mut pointer = Self {
            child,
            stdin,
            stdout,
        };
        pointer.read_line("ready");
        pointer
    }

    pub fn click(&mut self) {
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

pub struct Wheel {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Wheel {
    pub fn start(sway: &Sway) -> Self {
        let bin = compile_scroll(sway.name());
        let (child, stdin, stdout) = spawn_held(&bin, sway);
        let mut wheel = Self {
            child,
            stdin,
            stdout,
        };
        wheel.read_line("ready");
        wheel
    }

    pub fn scroll(&mut self, dy: i32) {
        writeln!(self.stdin, "scroll {dy}").expect("write a scroll");
        self.stdin.flush().expect("flush a scroll");
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

impl Drop for Wheel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Keyboard {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Keyboard {
    pub fn start(sway: &Sway) -> Self {
        let bin = compile_keyboard(sway.name());
        let (child, stdin, stdout) = spawn_held(&bin, sway);
        let mut keyboard = Self {
            child,
            stdin,
            stdout,
        };
        keyboard.read_line("ready");
        keyboard
    }

    pub fn type_text(&mut self, text: &str) {
        writeln!(self.stdin, "{text}").expect("write the text");
        self.stdin.flush().expect("flush the text");
        self.read_line("ok");
    }

    pub fn press_enter(&mut self) {
        writeln!(self.stdin, "\u{1}").expect("write enter");
        self.stdin.flush().expect("flush enter");
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

fn spawn_held(bin: &Path, sway: &Sway) -> (Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(bin)
        .arg("hold")
        .env("XDG_RUNTIME_DIR", sway.runtime())
        .env("WAYLAND_DISPLAY", sway.display())
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DEBUG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the virtual device");
    let stdin = child.stdin.take().expect("virtual device stdin");
    let stdout = BufReader::new(child.stdout.take().expect("virtual device stdout"));
    (child, stdin, stdout)
}

fn compile_click(name: &str) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/pointer");
    compile(
        name,
        "pointer-click",
        &src,
        "wlr-virtual-pointer-unstable-v1.xml",
        "pointer_click.c",
        "pointer_click",
        &["-lwayland-client"],
    )
}

fn compile_scroll(name: &str) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/pointer");
    compile(
        name,
        "pointer-scroll",
        &src,
        "wlr-virtual-pointer-unstable-v1.xml",
        "pointer_scroll.c",
        "pointer_scroll",
        &["-lwayland-client"],
    )
}

fn compile_keyboard(name: &str) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/keyboard");
    compile(
        name,
        "keyboard",
        &src,
        "virtual-keyboard-unstable-v1.xml",
        "keyboard_type.c",
        "keyboard_type",
        &["-lwayland-client", "-lxkbcommon"],
    )
}

fn compile(
    name: &str,
    label: &str,
    src_dir: &Path,
    xml_name: &str,
    source_name: &str,
    bin_name: &str,
    libs: &[&str],
) -> PathBuf {
    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{label}"));
    fs::create_dir_all(&out_dir).expect("create the tool build dir");
    let xml = src_dir.join(xml_name);
    let source = src_dir.join(source_name);
    let stem = xml
        .file_stem()
        .expect("protocol xml has a name")
        .to_string_lossy()
        .into_owned();
    let header = out_dir.join(format!("{stem}-client-protocol.h"));
    let protocol = out_dir.join(format!("{stem}-protocol.c"));
    let bin = out_dir.join(bin_name);
    let xml_arg = arg(&xml);
    let header_arg = arg(&header);
    let protocol_arg = arg(&protocol);
    let source_arg = arg(&source);
    let bin_arg = arg(&bin);
    let out_arg = arg(&out_dir);
    run_tool("wayland-scanner", &["client-header", &xml_arg, &header_arg]);
    run_tool(
        "wayland-scanner",
        &["private-code", &xml_arg, &protocol_arg],
    );
    let mut gcc_args = vec![
        "-O2",
        "-I",
        out_arg.as_str(),
        "-o",
        bin_arg.as_str(),
        source_arg.as_str(),
        protocol_arg.as_str(),
    ];
    gcc_args.extend(libs);
    run_tool("gcc", &gcc_args);
    bin
}

fn arg(path: &Path) -> String {
    path.to_str().expect("path is utf-8").to_string()
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
