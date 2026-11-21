use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use engine::{CpuFrame, Page};
use gpui::{
    Context, IntoElement, ObjectFit, Render, RenderImage, Window, div, img, prelude::*, rgb,
};

use crate::metrics::{self, Sampler};
use crate::present::copied_image;

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
        if self.sample.finished() {
            return;
        }
        if let Some(frame) = poll_page() {
            self.latest = Some(frame);
        }
        let Some(frame) = self.latest.clone() else {
            window.request_animation_frame();
            return;
        };
        self.image = Some(copied_image(&frame));
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
        window.request_animation_frame();
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        let mut root = div().size_full().bg(rgb(0xffffff));
        if let Some(image) = self.image.clone() {
            root = root.child(img(image).size_full().object_fit(ObjectFit::Fill));
        }
        root
    }
}

fn poll_page() -> Option<CpuFrame> {
    PAGE.with(|slot| slot.borrow_mut().as_mut().and_then(Page::poll))
}
