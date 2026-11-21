use gpui::{
    App, Application, Bounds, Context, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};

struct Blank;

impl Render for Blank {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xffffff))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                // X11 は RefCell を借りたままウィンドウを閉じ、 quit も同じ RefCell を借りる。
                // defer はまだ借用の中で動くので、借用が外れてから動く spawn で quit する。
                cx.spawn(async move |cx| {
                    cx.update(|cx| cx.quit()).ok();
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
            |_, cx| cx.new(|_| Blank),
        )
        .expect("open the susie window");
    });
}
