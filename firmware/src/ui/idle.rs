//! Idle screen — what the clock shows when no Pomodoro is running.
//!
//! Layout (32x8):
//!
//! ```text
//!     cols 0..=3    5..=16       18..=31
//!   +------------+------------+----------------+
//!   |     M      | minutes    | completed work |
//!   | (FONT_4X6) | today      | segment squares|
//!   +------------+------------+----------------+
//! ```
//!
//! The segment grid fits twelve completed work intervals. If today has more
//! completions than fit, the last square blinks white to indicate overflow.
//!
//! Buttons:
//!   * Press LEFT / SELECT / RIGHT → start the corresponding preset.
//!   * Long-press SELECT          → open the settings menu.

use embedded_graphics::{
    mono_font::{ascii::FONT_4X6, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::Point,
    text::{Baseline, Text},
    Drawable,
};

use crate::{
    display::{FrameBuffer, RgbColor},
    input::{ButtonEvent, ButtonId},
    ui::{RenderContext, UiAction},
};

const MINUTES_X: i32 = 5;
const SEGMENT_X0: usize = 18;
const SEGMENT_Y0: usize = 0;
const SEGMENT_COLS: usize = 4;
const SEGMENT_ROWS: usize = 3;
const SEGMENT_SIZE: usize = 2;
const SEGMENT_STRIDE: usize = 3;
const MAX_SEGMENT_SQUARES: usize = SEGMENT_COLS * SEGMENT_ROWS;

pub fn handle(ev: ButtonEvent, _ctx: &RenderContext<'_>) -> Option<UiAction> {
    match ev {
        ButtonEvent::Press(ButtonId::Left) => Some(UiAction::StartPreset(0)),
        ButtonEvent::Press(ButtonId::Select) => Some(UiAction::StartPreset(1)),
        ButtonEvent::Press(ButtonId::Right) => Some(UiAction::StartPreset(2)),
        ButtonEvent::LongPress(ButtonId::Select) => Some(UiAction::EnterMenu),
        _ => None,
    }
}

pub fn render(fb: &mut FrameBuffer, ctx: &RenderContext<'_>) {
    let today_stats = ctx
        .today
        .map(|t| ctx.stats.today(t))
        .unwrap_or_else(|| ctx.stats.current());
    let today_minutes = today_stats.focus_minutes;

    // "M" = productive minutes today.
    let label_style = MonoTextStyle::new(&FONT_4X6, Rgb888::new(160, 160, 160));
    let _ = Text::with_baseline("M", Point::new(0, 1), label_style, Baseline::Top).draw(fb);

    // Today's productive minutes.
    let mut buf = [0u8; 3];
    let text = format_u16(today_minutes, &mut buf);
    let count_color = if today_minutes > 0 {
        Rgb888::new(255, 200, 0)
    } else {
        Rgb888::new(100, 100, 100)
    };
    let count_style = MonoTextStyle::new(&FONT_4X6, count_color);
    let _ =
        Text::with_baseline(text, Point::new(MINUTES_X, 1), count_style, Baseline::Top).draw(fb);

    draw_segment_squares(fb, today_stats.completed_pomodoros, ctx.now_ms);
}

fn draw_segment_squares(fb: &mut FrameBuffer, completed: u16, now_ms: u64) {
    let visible = (completed as usize).min(MAX_SEGMENT_SQUARES);
    let overflow = completed as usize > MAX_SEGMENT_SQUARES;
    let overflow_on = (now_ms / 600) % 2 == 0;

    for i in 0..visible {
        let col = i % SEGMENT_COLS;
        let row = i / SEGMENT_COLS;
        let x0 = SEGMENT_X0 + col * SEGMENT_STRIDE;
        let y0 = SEGMENT_Y0 + row * SEGMENT_STRIDE;
        let color = if overflow && i + 1 == MAX_SEGMENT_SQUARES && overflow_on {
            RgbColor::WHITE
        } else {
            segment_color(i)
        };

        for y in y0..(y0 + SEGMENT_SIZE) {
            for x in x0..(x0 + SEGMENT_SIZE) {
                fb.set_pixel(x, y, color);
            }
        }
    }
}

fn segment_color(i: usize) -> RgbColor {
    const COLORS: [RgbColor; 6] = [
        RgbColor::new(255, 120, 0),
        RgbColor::new(255, 200, 0),
        RgbColor::new(80, 220, 80),
        RgbColor::new(0, 180, 220),
        RgbColor::new(120, 120, 255),
        RgbColor::new(220, 80, 180),
    ];
    COLORS[i % COLORS.len()]
}

fn format_u16(n: u16, buf: &mut [u8; 3]) -> &str {
    let n = n.min(999);
    if n >= 100 {
        buf[0] = b'0' + (n / 100) as u8;
        buf[1] = b'0' + ((n / 10) % 10) as u8;
        buf[2] = b'0' + (n % 10) as u8;
        unsafe { core::str::from_utf8_unchecked(&buf[..3]) }
    } else if n >= 10 {
        buf[0] = b'0' + (n / 10) as u8;
        buf[1] = b'0' + (n % 10) as u8;
        unsafe { core::str::from_utf8_unchecked(&buf[..2]) }
    } else {
        buf[0] = b'0' + n as u8;
        unsafe { core::str::from_utf8_unchecked(&buf[..1]) }
    }
}
