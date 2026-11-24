use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use cef::args::Args;
use cef::*;

use crate::frame::{CpuFrame, Frame as PresentedFrame, VIEW_HEIGHT, VIEW_WIDTH};
use crate::load::{LoadNotice, Loads, NavEvent};

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
    delivered: Cell<bool>,
    frame: RefCell<Option<CpuFrame>>,
    failure: RefCell<Option<String>>,
    browser: RefCell<Option<Browser>>,
    loads: RefCell<Loads>,
    nav_events: RefCell<Vec<NavEvent>>,
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
            delivered: Cell::new(false),
            frame: RefCell::new(None),
            failure: RefCell::new(None),
            browser: RefCell::new(None),
            loads: RefCell::new(Loads::new()),
            nav_events: RefCell::new(Vec::new()),
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

pub fn is_cef_subprocess() -> bool {
    std::env::args()
        .skip(1)
        .any(|arg| arg == "--type" || arg.starts_with("--type="))
}

pub fn run_cef_subprocess() -> Result<i32, SessionError> {
    match boot()? {
        Boot::Subprocess(exit_code) => Ok(exit_code),
        Boot::Browser(_) => Err(SessionError::Setup("これはブラウザプロセスです")),
    }
}

pub enum Opened {
    Subprocess { exit_code: i32 },
    Page(Page),
}

pub struct Page {
    app: App,
    state: Rc<State>,
}

impl Page {
    pub fn open(url: &str) -> Result<Opened, SessionError> {
        let BrowserBoot {
            args,
            mut app,
            state,
        } = match boot()? {
            Boot::Subprocess(exit_code) => return Ok(Opened::Subprocess { exit_code }),
            Boot::Browser(browser) => browser,
        };

        *state.url.borrow_mut() = url.to_string();
        state.loads.borrow_mut().request();
        let resources = resource_dir()?;
        let cache = cache_dir()?;
        let settings = Settings {
            no_sandbox: 0,
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
        Ok(Opened::Page(Page { app, state }))
    }

    /// CEF を 1 回進め、新しいフレームがあれば渡す。
    pub fn poll(&mut self) -> Option<CpuFrame> {
        do_message_loop_work();
        if self.state.loaded.get() && !self.state.nudged.get() {
            nudge(&self.state);
        }
        let frame = self.state.frame.borrow_mut().take();
        if frame.is_some() {
            self.state.delivered.set(true);
        }
        frame
    }

    /// ウィンドウ上の左クリックを、ページのこの位置へ渡す。
    /// `down` が true のときは押し、 false のときは離す。
    pub fn click(&mut self, x: i32, y: i32, down: bool, click_count: i32) {
        let browser = self.state.browser.borrow().clone();
        let Some(host) = browser.and_then(|browser| browser.host()) else {
            return;
        };
        let event = MouseEvent { x, y, modifiers: 0 };
        host.send_mouse_move_event(Some(&event), 0);
        host.send_mouse_click_event(
            Some(&event),
            MouseButtonType::LEFT,
            i32::from(!down),
            click_count,
        );
    }

    /// 確定した文字をページへ渡す。変換中の表示は扱わない。
    pub fn commit_text(&mut self, text: &str) {
        let browser = self.state.browser.borrow().clone();
        let Some(host) = browser.and_then(|browser| browser.host()) else {
            return;
        };
        // フォーカスがない OSR は、キーを描画側へ渡さない。
        host.set_focus(1);
        for ch in text.chars() {
            for event in committed_char_events(ch) {
                host.send_key_event(Some(&event));
            }
        }
    }

    /// 今のページを、この URL へ移す。戻した番号が、この読み込みの番号。
    /// ブラウザがまだ無いときは None で、読み込みは始まらない。
    /// 描画はここでは止めない。同じ文書内の移動や、読み込みが始まる前に拒否された URL は、完了の通知が来ない。
    pub fn load(&mut self, url: &str) -> Option<u64> {
        let browser = self.state.browser.borrow().clone()?;
        let frame = browser.main_frame()?;
        let generation = self.state.loads.borrow_mut().request();
        *self.state.url.borrow_mut() = url.to_string();
        let target = CefString::from(url);
        frame.load_url(Some(&target));
        Some(generation)
    }

    /// 起動時の読み込みに付けた番号。
    pub fn generation(&self) -> u64 {
        self.state.loads.borrow().issued()
    }

    /// `poll` までに届いた、読み込みの結果。
    pub fn take_nav(&mut self) -> Vec<NavEvent> {
        std::mem::take(&mut *self.state.nav_events.borrow_mut())
    }

    /// ウィンドウ上のホイールを、ページのこの位置へ渡す。
    pub fn scroll(&mut self, x: i32, y: i32, delta_x: i32, delta_y: i32) {
        if delta_x == 0 && delta_y == 0 {
            return;
        }
        let browser = self.state.browser.borrow().clone();
        let Some(host) = browser.and_then(|browser| browser.host()) else {
            return;
        };
        let event = MouseEvent { x, y, modifiers: 0 };
        host.send_mouse_move_event(Some(&event), 0);
        host.send_mouse_wheel_event(Some(&event), delta_x, delta_y);
    }

    pub fn shutdown(self) -> Result<CpuFrame, SessionError> {
        close_browser(&self.state);
        let close_start = Instant::now();
        while !self.state.closed.get() && close_start.elapsed() < CLOSE_TIMEOUT {
            do_message_loop_work();
            std::thread::sleep(Duration::from_millis(10));
        }
        self.state.browser.borrow_mut().take();
        shutdown();
        let result = take_frame(&self.state);
        drop(self.app);
        result
    }
}

pub fn render_fixture(fixture: &Path) -> Result<Rendered, SessionError> {
    let url = file_url(fixture)?;
    let page = match Page::open(&url)? {
        Opened::Subprocess { exit_code } => return Ok(Rendered::Subprocess { exit_code }),
        Opened::Page(page) => page,
    };
    pump_until(&page.state, PAINT_TIMEOUT);
    page.shutdown()
        .map(|frame| Rendered::Frame(PresentedFrame::Cpu(frame)))
}

fn take_frame(state: &State) -> Result<CpuFrame, SessionError> {
    if let Some(frame) = state.frame.borrow_mut().take() {
        return Ok(frame);
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

/// 確定した 1 文字を、 CEF のキーイベントにする。
/// 押下の仮想キーが分かる文字は、押し、文字、離しの順。
/// 分からない文字は、文字イベントだけにする。
/// 文字コードを押下に使うと、 `%` や `.` がカーソル移動や削除になる。
/// 制御文字と、 UTF-16 の 1 符号単位に収まらない文字は返さない。
fn committed_char_events(ch: char) -> Vec<KeyEvent> {
    let Some(unit) = u16::try_from(u32::from(ch)).ok() else {
        return Vec::new();
    };
    if ch.is_control() {
        return Vec::new();
    }
    let char_event = key_event(KeyEventType::CHAR, i32::from(unit), unit);
    let Some(key_code) = virtual_key(ch) else {
        return vec![char_event];
    };
    vec![
        key_event(KeyEventType::RAWKEYDOWN, key_code, 0),
        char_event,
        key_event(KeyEventType::KEYUP, key_code, 0),
    ]
}

/// 文字に対応する Windows の仮想キー。
/// 編集キー (Home や Left や Delete) の番号は返さない。
fn virtual_key(ch: char) -> Option<i32> {
    if ch.is_ascii_alphabetic() {
        return Some(ascii_code(ch.to_ascii_uppercase()));
    }
    if ch.is_ascii_digit() || ch == ' ' {
        return Some(ascii_code(ch));
    }
    // US キーボードの記号。数字キーか OEM キーで、文字コードとは別。
    Some(match ch {
        '!' => i32::from(b'1'),
        '@' => i32::from(b'2'),
        '#' => i32::from(b'3'),
        '$' => i32::from(b'4'),
        '%' => i32::from(b'5'),
        '^' => i32::from(b'6'),
        '&' => i32::from(b'7'),
        '*' => i32::from(b'8'),
        '(' => i32::from(b'9'),
        ')' => i32::from(b'0'),
        '-' | '_' => 0xBD,
        '=' | '+' => 0xBB,
        '[' | '{' => 0xDB,
        ']' | '}' => 0xDD,
        '\\' | '|' => 0xDC,
        ';' | ':' => 0xBA,
        '\'' | '"' => 0xDE,
        ',' | '<' => 0xBC,
        '.' | '>' => 0xBE,
        '/' | '?' => 0xBF,
        '`' | '~' => 0xC0,
        _ => return None,
    })
}

fn ascii_code(ch: char) -> i32 {
    i32::from(u8::try_from(u32::from(ch)).expect("ASCII は u8 に収まる"))
}

fn key_event(type_: KeyEventType, windows_key_code: i32, character: u16) -> KeyEvent {
    KeyEvent {
        type_,
        windows_key_code,
        character,
        unmodified_character: character,
        ..KeyEvent::default()
    }
}

fn note_main_load(state: &State, notice: LoadNotice, url: Option<String>) {
    let effect = state.loads.borrow_mut().notice(notice);
    let event = match effect {
        crate::load::LoadEffect::None => return,
        crate::load::LoadEffect::PageOpened { generation } => NavEvent::PageOpened {
            generation,
            url: url.unwrap_or_default(),
        },
        crate::load::LoadEffect::Succeeded { generation } => NavEvent::Succeeded { generation },
        crate::load::LoadEffect::Failed { generation } => NavEvent::Failed { generation },
    };
    state.nav_events.borrow_mut().push(event);
}

fn close_browser(state: &State) {
    let browser = state.browser.borrow().clone();
    if let Some(host) = browser.and_then(|browser| browser.host()) {
        host.close_browser(1);
    }
}

pub fn file_url(path: &Path) -> Result<String, SessionError> {
    let absolute = path.canonicalize().map_err(SessionError::Io)?;
    let text = absolute.to_str().ok_or(SessionError::Setup(
        "フィクスチャのパスが UTF-8 ではありません",
    ))?;
    Ok(format!("file://{text}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letter_is_keydown_char_and_keyup() {
        let events = committed_char_events('s');
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].type_, KeyEventType::RAWKEYDOWN);
        assert_eq!(events[0].windows_key_code, i32::from(b'S'));
        assert_eq!(events[0].character, 0);
        assert_eq!(events[1].type_, KeyEventType::CHAR);
        assert_eq!(events[1].windows_key_code, i32::from(b's'));
        assert_eq!(events[1].character, u16::from(b's'));
        assert_eq!(events[1].unmodified_character, u16::from(b's'));
        assert_eq!(events[2].type_, KeyEventType::KEYUP);
        assert_eq!(events[2].windows_key_code, i32::from(b'S'));
    }

    #[test]
    fn punctuation_keydown_is_not_an_editing_key() {
        let period = committed_char_events('.');
        assert_eq!(period[0].type_, KeyEventType::RAWKEYDOWN);
        assert_eq!(period[0].windows_key_code, 0xBE);
        assert_eq!(period[1].character, u16::from(b'.'));
        let percent = committed_char_events('%');
        assert_eq!(percent[0].windows_key_code, i32::from(b'5'));
        assert_ne!(percent[0].windows_key_code, 37);
        let dollar = committed_char_events('$');
        assert_eq!(dollar[0].windows_key_code, i32::from(b'4'));
        assert_ne!(dollar[0].windows_key_code, 36);
    }

    #[test]
    fn control_character_is_not_a_key() {
        assert!(committed_char_events('\n').is_empty());
    }

    #[test]
    fn ink_below_the_top_band_is_in_the_view() {
        let width = 200u32;
        let height = 220u32;
        let mut buf = white(width, height);
        paint_dark(&mut buf, width, 0, 200, 80);
        assert!(view_has_ink(&buf, width, height));
    }

    #[test]
    fn ink_at_the_top_is_in_the_view() {
        let width = 200u32;
        let height = 220u32;
        let mut buf = white(width, height);
        paint_dark(&mut buf, width, 0, 10, 80);
        assert!(view_has_ink(&buf, width, height));
    }

    #[test]
    fn blank_view_has_no_ink() {
        let buf = white(40, 40);
        assert!(!view_has_ink(&buf, 40, 40));
    }

    fn white(width: u32, height: u32) -> Vec<u8> {
        vec![255u8; (width as usize) * (height as usize) * 4]
    }

    fn paint_dark(buf: &mut [u8], width: u32, x: usize, y: usize, count: usize) {
        for offset in 0..count {
            let index = (y * width as usize + x + offset) * 4;
            buf[index] = 0;
            buf[index + 1] = 0;
            buf[index + 2] = 0;
        }
    }
}

fn view_has_ink(bgra: &[u8], width: u32, height: u32) -> bool {
    let width = width as usize;
    let rows = height as usize;
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

fn append_switches(command_line: &mut CommandLine) {
    for name in [
        "disable-gpu",
        "disable-gpu-compositing",
        "disable-dev-shm-usage",
        "use-alloy-style",
        "disable-breakpad",
    ] {
        command_line.append_switch(Some(&CefString::from(name)));
    }
    command_line.append_switch_with_value(
        Some(&CefString::from("ozone-platform")),
        Some(&CefString::from("headless")),
    );
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
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            let Some(frame) = frame else {
                return;
            };
            if frame.is_main() != 1 {
                return;
            }
            // 新しい文書の読み込みが始まった。終わるまで途中の描画は出さない。
            // 終わったあとは、最初のページと同じく invalidate で描き直す。
            self.state.loaded.set(false);
            self.state.nudged.set(false);
            let url = CefString::from(&frame.url()).to_string();
            note_main_load(&self.state, LoadNotice::Start, Some(url));
        }

        fn on_load_end(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _http_status_code: std::os::raw::c_int,
        ) {
            let Some(frame) = frame else {
                return;
            };
            if frame.is_main() != 1 {
                return;
            }
            self.state.loaded.set(true);
            note_main_load(&self.state, LoadNotice::End, None);
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
            note_main_load(
                &self.state,
                LoadNotice::Error {
                    aborted: error_code == Errorcode::ABORTED,
                },
                Some(url),
            );
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
            if !self.state.loaded.get() || !self.state.nudged.get() {
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
            if width as u32 != VIEW_WIDTH || height as u32 != VIEW_HEIGHT {
                return;
            }
            // 最初の 1 枚は、表示範囲のどこかに描画があるフレームにする。
            // 上端だけを見ると、余白の下にある本文を捨てて白いままになる。
            // 全体が白い描画はまだ確定しない。確定すると、その直後の本文を捨てる。
            // そのあとは、スクロールで全体が空白でも受ける。
            if !self.state.delivered.get()
                && (self.state.frame.borrow().is_some()
                    || !view_has_ink(bytes, width as u32, height as u32))
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
