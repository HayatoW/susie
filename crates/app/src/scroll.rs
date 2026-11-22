use gpui::{Pixels, Point, ScrollDelta};

use crate::click::{PageArea, page_point};

fn whole_pixels(value: Pixels) -> i32 {
    value.round().to_f64() as i32
}

pub struct PageWheel {
    pub x: i32,
    pub y: i32,
    pub delta_x: i32,
    pub delta_y: i32,
}

/// ページの表示領域のホイールを、 800x600 のページ座標と画素の移動量にする。
/// 下へ動かすデルタは負のまま渡す。 CEF は負の deltaY で下を見せる。
pub fn page_wheel(
    position: Point<Pixels>,
    area: PageArea,
    delta: ScrollDelta,
    line_height: Pixels,
) -> PageWheel {
    let pixels = delta.pixel_delta(line_height);
    let (x, y) = page_point(position, area);
    PageWheel {
        x,
        y,
        delta_x: whole_pixels(pixels.x),
        delta_y: whole_pixels(pixels.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn area(width: f32, height: f32) -> PageArea {
        PageArea {
            origin: point(px(0.), px(0.)),
            size: size(px(width), px(height)),
        }
    }

    #[test]
    fn wheel_down_stays_negative() {
        let wheel = page_wheel(
            point(px(400.), px(300.)),
            area(800., 600.),
            ScrollDelta::Lines(point(0., -3.)),
            px(20.),
        );
        assert_eq!(wheel.x, 400);
        assert_eq!(wheel.y, 300);
        assert_eq!(wheel.delta_x, 0);
        assert_eq!(wheel.delta_y, -60);
    }

    #[test]
    fn pixels_pass_through() {
        let wheel = page_wheel(
            point(px(100.), px(150.)),
            area(200., 300.),
            ScrollDelta::Pixels(point(px(12.), px(-34.2))),
            px(16.),
        );
        assert_eq!(wheel.x, 100);
        assert_eq!(wheel.y, 150);
        assert_eq!(wheel.delta_x, 12);
        assert_eq!(wheel.delta_y, -34);
    }
}
