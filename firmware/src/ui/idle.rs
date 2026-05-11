//! Idle screen — what the clock shows when no Pomodoro is running.
//!
//! Layout (32x8):
//!
//! ```text
//!     cols 0..=11         12..=20       21..=23  24..=30  31
//!   +----------------+----------------+--------+--------+---+
//!   |     TDY        | today's count  |  gap   | 7-day  | * |
//!   |  (FONT_4X6)    |  (FONT_4X6)    |        | bars   |   |
//!   +----------------+----------------+--------+--------+---+
//! ```
//!
//! The rightmost bar is "today" and blinks at ~1 Hz so the user can tell at
//! a glance which one is the current day. Older days are dimmer.
//!
//! Buttons:
//!   * Press LEFT / SELECT / RIGHT → start the corresponding preset.
//!   * Long-press SELECT          → open the settings menu.

use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_4X6},
    pixelcolor::Rgb888,
    prelude::Point,
    text::{Baseline, Text},
};

use crate::{
    display::{FrameBuffer, MATRIX_HEIGHT, RgbColor},
    input::{ButtonEvent, ButtonId},
    storage::stats::DayStats,
    ui::{RenderContext, UiAction},
};

const BAR_X0: usize = 24;
const BAR_COUNT: usize = 7;

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
    let today_count = ctx
        .today
        .map(|t| ctx.stats.today(t).completed_pomodoros)
        .unwrap_or(0);

    // "TDY"
    let label_style = MonoTextStyle::new(&FONT_4X6, Rgb888::new(160, 160, 160));
    let _ = Text::with_baseline("TDY", Point::new(0, 1), label_style, Baseline::Top).draw(fb);

    // today's count
    let mut buf = [0u8; 3];
    let text = format_u16(today_count, &mut buf);
    let count_color = if today_count > 0 {
        Rgb888::new(255, 200, 0)
    } else {
        Rgb888::new(100, 100, 100)
    };
    let count_style = MonoTextStyle::new(&FONT_4X6, count_color);
    let _ = Text::with_baseline(text, Point::new(14, 1), count_style, Baseline::Top).draw(fb);

    draw_bar_chart(fb, ctx);
}

fn draw_bar_chart(fb: &mut FrameBuffer, ctx: &RenderContext<'_>) {
    let week = ctx.stats.last_n(BAR_COUNT);
    let view = &week[..BAR_COUNT];

    let max = view
        .iter()
        .map(|d| d.completed_pomodoros)
        .max()
        .unwrap_or(0);

    let on = (ctx.now_ms / 600) % 2 == 0;
    let today_bright = RgbColor::new(255, 200, 0);
    let today_dim = RgbColor::new(120, 90, 0);
    let other = RgbColor::new(80, 80, 90);

    for (i, day) in view.iter().enumerate() {
        let x = BAR_X0 + i;
        if x >= 31 {
            break;
        }
        let is_today = i + 1 == BAR_COUNT;
        let height = bar_height(day, max);
        let color = if is_today {
            if on { today_bright } else { today_dim }
        } else {
            other
        };
        for h in 0..height {
            let y = MATRIX_HEIGHT - 1 - h;
            fb.set_pixel(x, y, color);
        }
        // Always mark today's column with at least a baseline pixel, so the
        // user can spot the "current day" even on a fresh ring with no data.
        if is_today && height == 0 && ctx.today.is_some() {
            fb.set_pixel(x, MATRIX_HEIGHT - 1, color);
        }
    }
}

/// Map a day's count to a bar height in `0..=8`, scaled against the local
/// maximum so the chart auto-ranges. Any non-zero count produces a bar at
/// least one pixel tall.
fn bar_height(day: &DayStats, max: u16) -> usize {
    if max == 0 || day.completed_pomodoros == 0 {
        return 0;
    }
    let scaled = (day.completed_pomodoros as u32 * MATRIX_HEIGHT as u32) / max as u32;
    (scaled as usize).max(1).min(MATRIX_HEIGHT)
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
