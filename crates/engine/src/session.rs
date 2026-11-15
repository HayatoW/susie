use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use cef::args::Args;
use cef::*;

use crate::frame::{CpuFrame, Frame as PresentedFrame, VIEW_HEIGHT, VIEW_WIDTH};

const PAINT_TIMEOUT: Duration = Duration::from_secs(90);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

pub enum Rendered {
    Subprocess { exit_code: i32 },
    Frame(PresentedFrame),
}

#[derive(Debug)]
pub enum SessionError {
    Setup(&'static str),
    Initialize(i32),
    BrowserProcess(i32),
    NoFrame { width: i32, height: i32 },
    Io(std::io::Error),
    Paint(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Setup(message) => write!(f, "{message}"),
            SessionError::Initialize(code) => {
                write!(f, "CEF の初期化に失敗しました。コードは {code} です")
            }
            SessionError::BrowserProcess(code) => {
                write!(f, "ブラウザプロセスの起動コードが {code} です")
            }
            SessionError::NoFrame { width, height } => write!(
                f,
                "フレームが届きませんでした。最後のペイントは {width}x{height} です"
            ),
            SessionError::Io(err) => write!(f, "{err}"),
            SessionError::Paint(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SessionError {}

struct State {
    url: RefCell<String>,
    started: Cell<bool>,
    loaded: Cell<bool>,
    nudged: Cell<bool>,
    closed: Cell<bool>,
    seen: Cell<(i32, i32)>,
    frame: RefCell<Option<CpuFrame>>,
    failure: RefCell<Option<String>>,
    browser: RefCell<Option<Browser>>,
}

impl State {
    fn new() -> Self {
        Self {
            url: RefCell::new(String::new()),
            started: Cell::new(false),
            loaded: Cell::new(false),
            nudged: Cell::new(false),
            closed: Cell::new(false),
            seen: Cell::new((0, 0)),
            frame: RefCell::new(None),
            failure: RefCell::new(None),
            browser: RefCell::new(None),
        }
    }

    fn finished(&self) -> bool {
        self.frame.borrow().is_some() || self.failure.borrow().is_some()
    }
}

struct BrowserBoot {
    args: Args,
    app: App,
    state: Rc<State>,
}

enum Boot {
    Subprocess(i32),
    Browser(BrowserBoot),
}

fn boot() -> Result<Boot, SessionError> {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let cmd = args
        .as_cmd_line()
        .ok_or(SessionError::Setup("コマンドラインを読めません"))?;
    let is_browser = cmd.has_switch(Some(&CefString::from("type"))) != 1;

    let state = Rc::new(State::new());
    let mut app = HostApp::new(state.clone());
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if !is_browser {
        return Ok(Boot::Subprocess(code));
    }
    if code != -1 {
        return Err(SessionError::BrowserProcess(code));
    }
    Ok(Boot::Browser(BrowserBoot { args, app, state }))
}

pub fn run_cef_subprocess() -> Result<i32, SessionError> {
    match boot()? {
        Boot::Subprocess(exit_code) => Ok(exit_code),
        Boot::Browser(_) => Err(SessionError::Setup("これはブラウザプロセスです")),
    }
}

pub fn render_fixture(fixture: &Path) -> Result<Rendered, SessionError> {
    let BrowserBoot {
        args,
        mut app,
        state,
    } = match boot()? {
        Boot::Subprocess(exit_code) => return Ok(Rendered::Subprocess { exit_code }),
        Boot::Browser(browser) => browser,
    };

    *state.url.borrow_mut() = file_url(fixture)?;
    let resources = resource_dir()?;
    let cache = cache_dir()?;
    let settings = Settings {
        no_sandbox: 1,
        windowless_rendering_enabled: 1,
        external_message_pump: 1,
        root_cache_path: cef_path(&cache)?,
        cache_path: cef_path(&cache)?,
        resources_dir_path: cef_path(&resources)?,
        locales_dir_path: cef_path(&resources.join("locales"))?,
        log_severity: LogSeverity::WARNING,
        ..Default::default()
    };
    let initialized = initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if initialized != 1 {
        return Err(SessionError::Initialize(initialized));
    }

    pump_until(&state, PAINT_TIMEOUT);
    close_browser(&state);
    let close_start = Instant::now();
    while !state.closed.get() && close_start.elapsed() < CLOSE_TIMEOUT {
        do_message_loop_work();
        std::thread::sleep(Duration::from_millis(10));
    }
    state.browser.borrow_mut().take();
    shutdown();

    if let Some(frame) = state.frame.borrow_mut().take() {
        return Ok(Rendered::Frame(PresentedFrame::Cpu(frame)));
    }
    if let Some(message) = state.failure.borrow().clone() {
        return Err(SessionError::Paint(message));
    }
    let (width, height) = state.seen.get();
    Err(SessionError::NoFrame { width, height })
}

fn pump_until(state: &State, limit: Duration) {
    let start = Instant::now();
    while !state.finished() && start.elapsed() < limit {
        do_message_loop_work();
        if state.loaded.get() && !state.nudged.get() {
            nudge(state);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn nudge(state: &State) {
    let browser = state.browser.borrow().clone();
    let Some(browser) = browser else {
        return;
    };
    let Some(host) = browser.host() else {
        return;
    };
    host.was_resized();
    host.invalidate(PaintElementType::VIEW);
    state.nudged.set(true);
}

fn close_browser(state: &State) {
    let browser = state.browser.borrow().clone();
    if let Some(host) = browser.and_then(|browser| browser.host()) {
        host.close_browser(1);
    }
}

fn file_url(path: &Path) -> Result<String, SessionError> {
    let absolute = path.canonicalize().map_err(SessionError::Io)?;
    let text = absolute.to_str().ok_or(SessionError::Setup(
        "フィクスチャのパスが UTF-8 ではありません",
    ))?;
    Ok(encode_file_url(text))
}

fn encode_file_url(path: &str) -> String {
    let mut url = String::from("file://");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                url.push(byte as char);
            }
            _ => {
                url.push('%');
                url.push(hex_digit(byte >> 4));
                url.push(hex_digit(byte & 0x0f));
            }
        }
    }
    url
}

fn hex_digit(nibble: u8) -> char {
    b"0123456789ABCDEF"[nibble as usize] as char
}

fn cache_dir() -> Result<PathBuf, SessionError> {
    let path = std::env::temp_dir().join("susie-cef-cache");
    std::fs::create_dir_all(&path).map_err(SessionError::Io)?;
    Ok(path)
}

fn resource_dir() -> Result<PathBuf, SessionError> {
    let dir = PathBuf::from(env!("SUSIE_CEF_DIR"));
    if !dir.join("icudtl.dat").is_file() {
        return Err(SessionError::Setup(
            "CEF のリソースが見つかりません。CEF_PATH を確認してください",
        ));
    }
    Ok(dir)
}

fn cef_path(path: &Path) -> Result<CefString, SessionError> {
    let text = path
        .to_str()
        .ok_or(SessionError::Setup("パスが UTF-8 ではありません"))?;
    Ok(CefString::from(text))
}

fn append_switches(command_line: &mut CommandLine) {
    for name in [
        "no-sandbox",
        "disable-gpu",
        "disable-gpu-compositing",
        "disable-dev-shm-usage",
        "use-alloy-style",
        "disable-breakpad",
        "allow-file-access-from-files",
    ] {
        command_line.append_switch(Some(&CefString::from(name)));
    }
    command_line.append_switch_with_value(
        Some(&CefString::from("ozone-platform")),
        Some(&CefString::from("headless")),
    );
}

fn top_has_ink(bgra: &[u8], width: u32, height: u32) -> bool {
    let width = width as usize;
    let rows = (height as usize).min(180);
    let columns = width.saturating_sub(24);
    let mut dark = 0usize;
    for y in 0..rows {
        let row = y * width * 4;
        for x in 0..columns {
            let index = row + x * 4;
            let pixel = &bgra[index..index + 4];
            if pixel[0] < 240 || pixel[1] < 240 || pixel[2] < 240 {
                dark += 1;
                if dark >= 80 {
                    return true;
                }
            }
        }
    }
    false
}

wrap_app! {
    struct HostApp {
        state: Rc<State>,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(command_line) = command_line else {
                return;
            };
            append_switches(command_line);
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(HostBrowserProcess::new(self.state.clone()))
        }
    }
}

wrap_browser_process_handler! {
    struct HostBrowserProcess {
        state: Rc<State>,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            if self.state.started.replace(true) {
                return;
            }
            let url = CefString::from(self.state.url.borrow().as_str());
            let window_info = WindowInfo {
                runtime_style: RuntimeStyle::ALLOY,
                ..Default::default()
            }
            .set_as_windowless(0);
            let mut client = HostClient::new(
                HostRender::new(self.state.clone()),
                HostLoad::new(self.state.clone()),
                HostLife::new(self.state.clone()),
            );
            let browser_settings = BrowserSettings {
                windowless_frame_rate: 30,
                background_color: 0xFFFF_FFFF,
                sans_serif_font_family: CefString::from("Noto Sans"),
                standard_font_family: CefString::from("Noto Sans"),
                ..Default::default()
            };
            let Some(browser) = browser_host_create_browser_sync(
                Some(&window_info),
                Some(&mut client),
                Some(&url),
                Some(&browser_settings),
                None,
                None,
            ) else {
                *self.state.failure.borrow_mut() = Some("ブラウザを作れませんでした".to_string());
                return;
            };
            if let Some(host) = browser.host() {
                host.was_resized();
            }
            *self.state.browser.borrow_mut() = Some(browser);
        }

        fn on_schedule_message_pump_work(&self, _delay_ms: i64) {}
    }
}

wrap_client! {
    struct HostClient {
        render_handler: RenderHandler,
        load_handler: LoadHandler,
        life_span_handler: LifeSpanHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render_handler.clone())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.load_handler.clone())
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life_span_handler.clone())
        }
    }
}

wrap_life_span_handler! {
    struct HostLife {
        state: Rc<State>,
    }

    impl LifeSpanHandler {
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            self.state.closed.set(true);
        }

        fn do_close(&self, _browser: Option<&mut Browser>) -> std::os::raw::c_int {
            0
        }
    }
}

wrap_load_handler! {
    struct HostLoad {
        state: Rc<State>,
    }

    impl LoadHandler {
        fn on_load_end(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _http_status_code: std::os::raw::c_int,
        ) {
            let Some(frame) = frame else {
                return;
            };
            if frame.is_main() == 1 {
                self.state.loaded.set(true);
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let main = frame.as_ref().is_none_or(|frame| frame.is_main() == 1);
            if !main {
                return;
            }
            let text = error_text.map(CefString::to_string).unwrap_or_default();
            let url = failed_url.map(CefString::to_string).unwrap_or_default();
            *self.state.failure.borrow_mut() =
                Some(format!("読み込みに失敗しました。{error_code:?} {text} {url}"));
        }
    }
}

wrap_render_handler! {
    struct HostRender {
        state: Rc<State>,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            let Some(rect) = rect else {
                return;
            };
            rect.x = 0;
            rect.y = 0;
            rect.width = VIEW_WIDTH as _;
            rect.height = VIEW_HEIGHT as _;
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> std::os::raw::c_int {
            let Some(screen_info) = screen_info else {
                return 0;
            };
            screen_info.device_scale_factor = 1.0;
            screen_info.depth = 24;
            screen_info.depth_per_component = 8;
            screen_info.is_monochrome = 0;
            let bounds = Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            };
            screen_info.rect = bounds;
            screen_info.available_rect = screen_info.rect.clone();
            1
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: std::os::raw::c_int,
            height: std::os::raw::c_int,
        ) {
            if type_ != PaintElementType::VIEW {
                return;
            }
            self.state.seen.set((width, height));
            if !self.state.loaded.get() || self.state.frame.borrow().is_some() {
                return;
            }
            if buffer.is_null() || width <= 0 || height <= 0 {
                return;
            }
            let Some(len) = (width as usize)
                .checked_mul(height as usize)
                .and_then(|pixels| pixels.checked_mul(4))
            else {
                return;
            };
            // SAFETY: CEF は、この呼び出しのあいだ、バッファを BGRA の width * height * 4 バイトとして有効に保つ。
            let bytes = unsafe { std::slice::from_raw_parts(buffer, len) };
            if width as u32 != VIEW_WIDTH
                || height as u32 != VIEW_HEIGHT
                || !top_has_ink(bytes, width as u32, height as u32)
            {
                return;
            }
            match CpuFrame::copy_bgra(width as u32, height as u32, bytes) {
                Ok(frame) => *self.state.frame.borrow_mut() = Some(frame),
                Err(err) => *self.state.failure.borrow_mut() = Some(err.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_file_url, file_url, top_has_ink};

    fn paint(width: usize, height: usize, pixels: &[(usize, usize)]) -> Vec<u8> {
        let mut bgra = vec![255u8; width * height * 4];
        for &(x, y) in pixels {
            let index = (y * width + x) * 4;
            bgra[index] = 0;
        }
        bgra
    }

    #[test]
    fn ink_ignores_the_scrollbar() {
        let width = 40;
        let height = 10;
        let pixels: Vec<_> = (0..height)
            .flat_map(|y| (width - 16..width).map(move |x| (x, y)))
            .collect();
        let bgra = paint(width, height, &pixels);
        assert!(!top_has_ink(&bgra, width as u32, height as u32));
    }

    #[test]
    fn ink_sees_text_in_the_top() {
        let width = 40;
        let height = 10;
        let pixels: Vec<_> = (0..80).map(|n| (n % 16, n / 16)).collect();
        let bgra = paint(width, height, &pixels);
        assert!(top_has_ink(&bgra, width as u32, height as u32));
    }

    #[test]
    fn ink_ignores_pixels_below_the_top() {
        let width = 40;
        let height = 200;
        let pixels: Vec<_> = (180..height)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .collect();
        let bgra = paint(width, height, &pixels);
        assert!(!top_has_ink(&bgra, width as u32, height as u32));
    }

    #[test]
    fn file_url_encodes_reserved_characters() {
        let dir = std::env::temp_dir().join(format!("susie-url-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a%b #?.html");
        std::fs::write(&path, b"x").unwrap();
        let url = file_url(&path).unwrap();
        let canonical = path.canonicalize().unwrap();
        assert_eq!(url, encode_file_url(canonical.to_str().unwrap()));
        assert!(!url.contains('#'));
        assert!(!url.contains('?'));
        assert!(!url.contains(' '));
        assert!(url.contains("%23"));
        assert!(url.contains("%3F"));
        assert!(url.contains("%20"));
        assert!(url.contains("%25"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
