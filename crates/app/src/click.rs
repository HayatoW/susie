use engine::{VIEW_HEIGHT, VIEW_WIDTH};
use gpui::{Pixels, Point, Size};

pub struct PageClick {
    pub x: i32,
    pub y: i32,
}

/// ページが見えている範囲。アドレスバーの下を指す。
#[derive(Clone, Copy, Debug)]
pub struct PageArea {
    pub origin: Point<Pixels>,
    pub size: Size<Pixels>,
}

/// ページの表示領域の位置を、 800x600 のページ座標にする。
/// フレームは欄の下にそのままの画素で出る。見えている位置が、ページの位置。
pub fn page_point(position: Point<Pixels>, area: PageArea) -> (i32, i32) {
    (
        view_pixel(position.x - area.origin.x, VIEW_WIDTH),
        view_pixel(position.y - area.origin.y, VIEW_HEIGHT),
    )
}

/// ページの表示領域のクリックを、 800x600 のページ座標にする。
pub fn page_click(position: Point<Pixels>, area: PageArea) -> PageClick {
    let (x, y) = page_point(position, area);
    PageClick { x, y }
}

fn view_pixel(position: Pixels, view: u32) -> i32 {
    let last = i32::try_from(view).expect("ビューの幅は i32 に収まる") - 1;
    (position.round().to_f64() as i32).clamp(0, last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn area(x: f32, y: f32, width: f32, height: f32) -> PageArea {
        PageArea {
            origin: point(px(x), px(y)),
            size: size(px(width), px(height)),
        }
    }

    #[test]
    fn click_keeps_the_page_pixel() {
        let click = page_click(point(px(100.), px(150.)), area(0., 0., 200., 300.));
        assert_eq!(click.x, 100);
        assert_eq!(click.y, 150);
    }

    #[test]
    fn click_uses_the_page_area_below_the_bar() {
        let click = page_click(point(px(400.), px(332.)), area(0., 64., 800., 536.));
        assert_eq!(click.x, 400);
        assert_eq!(click.y, 268);
    }

    #[test]
    fn click_clamps_to_the_view() {
        let click = page_click(point(px(-10.), px(1000.)), area(0., 0., 800., 600.));
        assert_eq!(click.x, 0);
        assert_eq!(click.y, 599);
    }
}
