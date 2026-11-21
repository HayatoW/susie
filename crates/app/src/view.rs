use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use engine::{CpuFrame, Page};
use gpui::{
    Context, FocusHandle, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseUpEvent,
    ObjectFit, Render, RenderImage, ScrollWheelEvent, Window, div, img, prelude::*, rgb,
};

use crate::click::{PageClick, page_click};
use crate::metrics::{self, Sampler};
use crate::present::copied_image;
use crate::scroll::{PageWheel, page_wheel};
use crate::text::committed_text;

thread_local! {
    static PAGE: RefCell<Option<Page>> = const { RefCell::new(None) };
}

pub fn install_page(page: Page) {
    PAGE.with(|slot| *slot.borrow_mut() = Some(page));
}

pub fn close_page() {
    PAGE.with(|slot| {
        if let Some(page) = slot.borrow_mut().take() {
            let _ = page.shutdown();
        }
    });
}

pub struct Browser {
    pub focus: FocusHandle,
    latest: Option<CpuFrame>,
    image: Option<Arc<RenderImage>>,
    // 直前のフレームがサンプルした画像。次の tick までアトラスに残す。
    retired: Option<Arc<RenderImage>>,
    sample: Sampler,
}

impl Browser {
    pub fn new(started: Instant, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            latest: None,
            image: None,
            retired: None,
            sample: Sampler::new(started),
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // draw は、今回の送信のあとで、前回の GPU 完了を待つ。
        // その待ちが終わった次の tick で、前回使った画像を外す。
        if let Some(retired) = self.retired.take() {
            let _ = window.drop_image(retired);
        }
        let fresh = poll_page();
        if let Some(frame) = fresh.clone() {
            self.latest = Some(frame);
        }
        let measuring = !self.sample.finished();
        let present = if measuring {
            self.latest.clone()
        } else {
            fresh
        };
        if let Some(frame) = present {
            self.replace_image(copied_image(&frame));
            if measuring {
                let entity = cx.entity().clone();
                window.on_next_frame(move |_window, cx| {
                    entity.update(cx, |this, _cx| {
                        let Some(sample) = this.sample.observe(Instant::now()) else {
                            return;
                        };
                        if let Err(err) = metrics::append_sample(sample) {
                            eprintln!("計測を書けません。{err}");
                        }
                    });
                });
            }
        }
        // 計測のあとも CEF を回さないと、入力のあとの描画が届かない。
        window.request_animation_frame();
    }

    fn replace_image(&mut self, image: Arc<RenderImage>) {
        if let Some(previous) = self.image.replace(image) {
            // 今外すと、直前の draw がまだサンプルしているテクスチャを消す。
            self.retired = Some(previous);
        }
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        let mut root = div()
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(0xffffff))
            .on_key_down(cx.listener(|_this, event: &KeyDownEvent, _window, _cx| {
                let Some(text) = committed_text(&event.keystroke) else {
                    return;
                };
                commit_page(&text);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, event: &MouseDownEvent, window, _cx| {
                    click_page(
                        page_click(event.position, window.viewport_size()),
                        true,
                        event.click_count,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_this, event: &MouseUpEvent, window, _cx| {
                    click_page(
                        page_click(event.position, window.viewport_size()),
                        false,
                        event.click_count,
                    );
                }),
            )
            .on_scroll_wheel(cx.listener(|_this, event: &ScrollWheelEvent, window, _cx| {
                scroll_page(page_wheel(
                    event.position,
                    window.viewport_size(),
                    event.delta,
                    window.line_height(),
                ));
            }));
        if let Some(image) = self.image.clone() {
            root = root.child(img(image).size_full().object_fit(ObjectFit::Fill));
        }
        root
    }
}

fn poll_page() -> Option<CpuFrame> {
    PAGE.with(|slot| slot.borrow_mut().as_mut().and_then(Page::poll))
}

fn click_page(click: PageClick, down: bool, click_count: usize) {
    let count = i32::try_from(click_count).expect("クリック回数は i32 に収まる");
    PAGE.with(|slot| {
        if let Some(page) = slot.borrow_mut().as_mut() {
            page.click(click.x, click.y, down, count);
        }
    });
}

fn commit_page(text: &str) {
    PAGE.with(|slot| {
        if let Some(page) = slot.borrow_mut().as_mut() {
            page.commit_text(text);
        }
    });
}

fn scroll_page(wheel: PageWheel) {
    if wheel.delta_x == 0 && wheel.delta_y == 0 {
        return;
    }
    PAGE.with(|slot| {
        if let Some(page) = slot.borrow_mut().as_mut() {
            page.scroll(wheel.x, wheel.y, wheel.delta_x, wheel.delta_y);
        }
    });
}
