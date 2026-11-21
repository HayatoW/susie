use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use engine::{CpuFrame, Page};
use gpui::{
    Context, IntoElement, ObjectFit, Render, RenderImage, ScrollWheelEvent, Window, div, img,
    prelude::*, rgb,
};

use crate::metrics::{self, Sampler};
use crate::present::copied_image;
use crate::scroll::{PageWheel, page_wheel};

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
    latest: Option<CpuFrame>,
    image: Option<Arc<RenderImage>>,
    sample: Sampler,
}

impl Browser {
    pub fn new(started: Instant) -> Self {
        Self {
            latest: None,
            image: None,
            sample: Sampler::new(started),
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            self.replace_image(window, copied_image(&frame));
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
        // 計測のあとも CEF を回さないと、スクロール後の描画が届かない。
        window.request_animation_frame();
    }

    fn replace_image(&mut self, window: &mut Window, image: Arc<RenderImage>) {
        if let Some(previous) = self.image.replace(image) {
            // RenderImage は毎回別の ID になる。残すとアトラスが増え続ける。
            let _ = window.drop_image(previous);
        }
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        let mut root = div()
            .size_full()
            .bg(rgb(0xffffff))
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
