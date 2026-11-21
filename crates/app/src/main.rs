mod metrics;
mod present;
mod scroll;
mod view;

use std::path::PathBuf;
use std::time::Instant;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size,
};

use crate::view::{Browser, close_page, install_page};

fn main() {
    let started = Instant::now();
    if engine::is_cef_subprocess() {
        match engine::run_cef_subprocess() {
            Ok(exit_code) => std::process::exit(exit_code),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/first-page.html");
    let page = match engine::Page::open(&fixture) {
        Ok(engine::Opened::Page(page)) => page,
        Ok(engine::Opened::Subprocess { exit_code }) => std::process::exit(exit_code),
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    install_page(page);

    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                // X11 は RefCell を借りたままウィンドウを閉じ、 quit も同じ RefCell を借りる。
                // defer はまだ借用の中で動くので、借用が外れてから動く spawn で quit する。
                cx.spawn(async move |cx| {
                    cx.update(|cx| {
                        close_page();
                        cx.quit();
                    })
                    .ok();
                })
                .detach();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(800.), px(600.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("susie".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| cx.new(|_| Browser::new(started)),
        )
        .expect("open the susie window");
    });
    close_page();
}
