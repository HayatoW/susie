use engine::{VIEW_HEIGHT, VIEW_WIDTH};
use gpui::{Pixels, Point, ScrollDelta, Size};

fn whole_pixels(value: Pixels) -> i32 {
    value.round().to_f64() as i32
}

pub struct PageWheel {
    pub x: i32,
    pub y: i32,
    pub delta_x: i32,
    pub delta_y: i32,
}

/// ウィンドウのホイールを、 800x600 のページ座標と画素の移動量にする。
/// 下へ動かすデルタは負のまま渡す。 CEF は負の deltaY で下を見せる。
pub fn page_wheel(
    position: Point<Pixels>,
    viewport: Size<Pixels>,
    delta: ScrollDelta,
    line_height: Pixels,
) -> PageWheel {
    let pixels = delta.pixel_delta(line_height);
    PageWheel {
        x: map_axis(position.x, viewport.width, VIEW_WIDTH),
        y: map_axis(position.y, viewport.height, VIEW_HEIGHT),
        delta_x: whole_pixels(pixels.x),
        delta_y: whole_pixels(pixels.y),
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
    fn wheel_down_stays_negative() {
        let wheel = page_wheel(
            point(px(400.), px(300.)),
            size(px(800.), px(600.)),
            ScrollDelta::Lines(point(0., -3.)),
            px(20.),
        );
        assert_eq!(wheel.x, 400);
        assert_eq!(wheel.y, 300);
        assert_eq!(wheel.delta_x, 0);
        assert_eq!(wheel.delta_y, -60);
    }

    #[test]
    fn pixels_pass_through_and_position_scales() {
        let wheel = page_wheel(
            point(px(100.), px(150.)),
            size(px(200.), px(300.)),
            ScrollDelta::Pixels(point(px(12.), px(-34.2))),
            px(16.),
        );
        assert_eq!(wheel.x, 400);
        assert_eq!(wheel.y, 300);
        assert_eq!(wheel.delta_x, 12);
        assert_eq!(wheel.delta_y, -34);
    }
}
