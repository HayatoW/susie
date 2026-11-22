use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use engine::{CpuFrame, Page};
use gpui::{
    Context, Div, FocusHandle, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseUpEvent, ObjectFit, Pixels, Render, RenderImage, ScrollWheelEvent, Size, Window, div, img,
    point, prelude::*, px, rgb, size,
};

use crate::address::{self, Address};
use crate::click::{PageArea, PageClick, page_click};
use crate::metrics::{self, InputLatency, Sampler};
use crate::present::copied_image;
use crate::scroll::{PageWheel, page_wheel};
use crate::text::committed_text;

// 入力欄の文字は、録画でも読める大きさにする。
const BAR_HEIGHT: f32 = 80.;
const BAR_FONT: f32 = 48.;

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
    pub page_focus: FocusHandle,
    bar_focus: FocusHandle,
    address: Address,
    latest: Option<CpuFrame>,
    image: Option<Arc<RenderImage>>,
    // 直前のフレームがサンプルした画像。次の tick までアトラスに残す。
    retired: Option<Arc<RenderImage>>,
    sample: Sampler,
    input: InputLatency,
    // 最初の左ボタン押下の時点で見えていたページ。
    input_baseline: Option<CpuFrame>,
}

impl Browser {
    pub fn new(started: Instant, url: &str, cx: &mut Context<Self>) -> Self {
        Self {
            page_focus: cx.focus_handle(),
            bar_focus: cx.focus_handle(),
            address: Address::new(url),
            latest: None,
            image: None,
            retired: None,
            sample: Sampler::new(started),
            input: InputLatency::new(),
            input_baseline: None,
        }
    }

    /// ページ上の最初の左ボタン押下を、入力の遅れの始まりにする。
    fn arm_page_press(&mut self) {
        if !self.input.press(Instant::now()) {
            return;
        }
        self.input_baseline = self.latest.clone();
    }

    /// 画素が変わる前のスクロールでは、その描画を押下の遅れにしない。
    fn drop_unsettled_press(&mut self) {
        if self.input.cancel() {
            self.input_baseline = None;
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
            // 画素が変わったフレームも、起動のサンプルと同じく、次のフレームで載せた時刻を使う。
            let changed = self.input.see(frame_differs(&self.input_baseline, &frame));
            self.replace_image(copied_image(&frame));
            if measuring || changed {
                let entity = cx.entity().clone();
                window.on_next_frame(move |_window, cx| {
                    entity.update(cx, |this, _cx| {
                        let now = Instant::now();
                        if let Some(sample) = this.sample.observe(now)
                            && let Err(err) = metrics::append_sample(sample)
                        {
                            eprintln!("計測を書けません。{err}");
                        }
                        if let Some(input_ms) = this.input.present(now) {
                            this.input_baseline = None;
                            if let Err(err) = metrics::append_input_ms(input_ms) {
                                eprintln!("計測を書けません。{err}");
                            }
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

    fn address_bar(&self, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .h(px(BAR_HEIGHT))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .px(px(20.))
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family("Noto Sans")
            .text_size(px(BAR_FONT))
            .text_color(rgb(0x111111))
            .bg(rgb(0xffffff))
            .track_focus(&self.bar_focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, window, _cx| {
                    this.address.focus_from_click();
                    window.focus(&this.bar_focus);
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, _cx| {
                let Some(key) = address::address_key(&event.keystroke) else {
                    return;
                };
                if let Some(url) = this.address.on_key(key) {
                    load_page(&url);
                }
            }));
        let text = self.address.shown().to_string();
        // クリックした入力欄は、選択かキャレットでキーの行き先が分かる。
        if self.address.replaces() && !text.is_empty() {
            bar = bar.child(div().bg(rgb(0xd6d6d6)).child(text));
        } else {
            if !text.is_empty() {
                bar = bar.child(text);
            }
            if self.address.focused() {
                bar = bar.child(div().w(px(2.)).h(px(BAR_FONT)).bg(rgb(0x111111)));
            }
        }
        bar
    }

    fn page_surface(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let area = page_area(window.viewport_size());
        let mut page = div()
            .w(area.size.width)
            .h(area.size.height)
            .overflow_hidden()
            .track_focus(&self.page_focus)
            .on_key_down(cx.listener(|_this, event: &KeyDownEvent, _window, _cx| {
                let Some(text) = committed_text(&event.keystroke) else {
                    return;
                };
                commit_page(&text);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, _cx| {
                    this.arm_page_press();
                    this.address.blur_to_page();
                    window.focus(&this.page_focus);
                    click_page(
                        page_click(event.position, page_area(window.viewport_size())),
                        true,
                        event.click_count,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_this, event: &MouseUpEvent, window, _cx| {
                    click_page(
                        page_click(event.position, page_area(window.viewport_size())),
                        false,
                        event.click_count,
                    );
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, _cx| {
                let wheel = page_wheel(
                    event.position,
                    page_area(window.viewport_size()),
                    event.delta,
                    window.line_height(),
                );
                if wheel.delta_x != 0 || wheel.delta_y != 0 {
                    this.drop_unsettled_press();
                }
                scroll_page(wheel);
            }));
        if let Some(image) = self.image.clone() {
            page = page.child(img(image).size_full().object_fit(ObjectFit::Fill));
        }
        page
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        let bar = self.address_bar(cx);
        let page = self.page_surface(window, cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0xffffff))
            .child(bar)
            .child(page)
    }
}

fn page_area(viewport: Size<Pixels>) -> PageArea {
    let bar = px(BAR_HEIGHT);
    let height = if viewport.height > bar {
        viewport.height - bar
    } else {
        px(1.)
    };
    PageArea {
        origin: point(px(0.), bar),
        size: size(viewport.width, height),
    }
}

/// 押下の時点で見えていた画素と違うか。まだフレームが無い押下は、最初のフレームを違いにする。
fn frame_differs(baseline: &Option<CpuFrame>, frame: &CpuFrame) -> bool {
    baseline.as_ref().is_none_or(|shown| shown != frame)
}

fn poll_page() -> Option<CpuFrame> {
    PAGE.with(|slot| slot.borrow_mut().as_mut().and_then(Page::poll))
}

fn load_page(url: &str) {
    PAGE.with(|slot| {
        if let Some(page) = slot.borrow_mut().as_mut() {
            page.load(url);
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_pixels_are_not_a_change() {
        let frame = CpuFrame::copy_bgra(1, 1, &[1, 2, 3, 4]).unwrap();
        let again = CpuFrame::copy_bgra(1, 1, &[1, 2, 3, 4]).unwrap();
        assert!(!frame_differs(&Some(frame), &again));
    }

    #[test]
    fn different_pixels_are_a_change() {
        let frame = CpuFrame::copy_bgra(1, 1, &[1, 2, 3, 4]).unwrap();
        let next = CpuFrame::copy_bgra(1, 1, &[9, 2, 3, 4]).unwrap();
        assert!(frame_differs(&Some(frame), &next));
    }

    #[test]
    fn a_press_before_any_frame_treats_the_first_frame_as_a_change() {
        let frame = CpuFrame::copy_bgra(1, 1, &[1, 2, 3, 4]).unwrap();
        assert!(frame_differs(&None, &frame));
    }
}
