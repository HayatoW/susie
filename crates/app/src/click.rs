use engine::{VIEW_HEIGHT, VIEW_WIDTH};
use gpui::{Pixels, Point, Size};

pub struct PageClick {
    pub x: i32,
    pub y: i32,
}

/// ウィンドウ上の位置を、 800x600 のページ座標にする。
pub fn page_click(position: Point<Pixels>, viewport: Size<Pixels>) -> PageClick {
    PageClick {
        x: map_axis(position.x, viewport.width, VIEW_WIDTH),
        y: map_axis(position.y, viewport.height, VIEW_HEIGHT),
    }
}

fn map_axis(position: Pixels, viewport: Pixels, view: u32) -> i32 {
    let scaled = position.to_f64() / viewport.to_f64() * f64::from(view);
    let last = i32::try_from(view).expect("ビューの幅は i32 に収まる") - 1;
    (scaled.round() as i32).clamp(0, last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn click_scales_to_the_page() {
        let click = page_click(point(px(100.), px(150.)), size(px(200.), px(300.)));
        assert_eq!(click.x, 400);
        assert_eq!(click.y, 300);
    }

    #[test]
    fn click_clamps_to_the_view() {
        let click = page_click(point(px(-10.), px(1000.)), size(px(800.), px(600.)));
        assert_eq!(click.x, 0);
        assert_eq!(click.y, 599);
    }
}
