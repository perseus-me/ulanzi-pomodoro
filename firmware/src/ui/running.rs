//! Running / paused session screen.
//!
//! Layout (32x8). `cols 0..=24` host the 5x8 `MM:SS` countdown. The top-right
//! 4x6 corner shows a `W` / `R` phase letter, and the 7x2 stripe at the
//! bottom-right is filled left-to-right as the phase progresses.
//!
//! ```text
//!     cols 0..=24            25..=27      28..=31
//!   +---------------------+------------+-----------+
//!   |   M  M  :  S  S    |            |  W or R   |   rows 0..=5
//!   |                    |            |           |
//!   |                    |####progress bar#####   |   rows 6..=7
//!   +---------------------+------------+-----------+
//! ```
//!
//! Colours: red while in `Work`, green while in `Rest`, dimmed and slowly
//! blinking while paused.

use embedded_graphics::{
    Drawable,
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_4X6, FONT_5X8},
    },
    pixelcolor::Rgb888,
    prelude::Point,
    text::{Baseline, Text},
};

use crate::{
    display::{FrameBuffer, MATRIX_HEIGHT, MATRIX_WIDTH, RgbColor},
    input::{ButtonEvent, ButtonId},
    pomodoro::{Phase, State},
    ui::{RenderContext, UiAction},
};

/// Horizontal progress bar: columns 25..=31, rows 6..=7.
const PROGRESS_X0: usize = 25;
const PROGRESS_X1: usize = MATRIX_WIDTH;
const PROGRESS_Y0: usize = 6;
const PROGRESS_Y1: usize = MATRIX_HEIGHT;
const PROGRESS_WIDTH: usize = PROGRESS_X1 - PROGRESS_X0;

pub fn handle(ev: ButtonEvent, ctx: &RenderContext<'_>) -> Option<UiAction> {
    match ev {
        ButtonEvent::Press(_) => {
            if matches!(
                ctx.pomodoro.state(),
                State::Running { .. } | State::Paused { .. }
            ) {
                Some(UiAction::TogglePause)
            } else {
                None
            }
        }
        ButtonEvent::LongPress(ButtonId::Select) => match ctx.pomodoro.phase() {
            Some(Phase::Rest) => Some(UiAction::SkipRest),
            Some(Phase::Work) | None => Some(UiAction::StopSession),
        },
        _ => None,
    }
}

pub fn render(fb: &mut FrameBuffer, ctx: &RenderContext<'_>) {
    let pomodoro = ctx.pomodoro;
    let now_ms = ctx.now_ms;
    let phase = pomodoro.phase().unwrap_or(Phase::Work);
    let remaining_ms = pomodoro.remaining_ms(now_ms).unwrap_or(0);
    let duration_ms = pomodoro.duration_ms().unwrap_or(remaining_ms.max(1));
    let paused = pomodoro.is_paused();

    let base = base_color(phase);
    let text_color = if paused {
        // Slow blink while paused so the inactive state is obvious.
        let on = (now_ms / 600) % 2 == 0;
        base.scale(if on { 110 } else { 35 })
    } else {
        base
    };

    // MM:SS, rounded up so the last second is visible for a full tick.
    let total_seconds = remaining_ms.div_ceil(1_000);
    let mm = ((total_seconds / 60) as u16).min(99);
    let ss = (total_seconds % 60) as u8;

    let mut buf = [0u8; 5];
    let text = format_mmss(mm, ss, &mut buf);

    let time_style = MonoTextStyle::new(&FONT_5X8, Rgb888::from(text_color));
    let _ = Text::with_baseline(text, Point::new(0, 0), time_style, Baseline::Top).draw(fb);

    // Phase letter in the top-right 4x6 cell.
    let letter = match phase {
        Phase::Work => "W",
        Phase::Rest => "R",
    };
    let letter_style = MonoTextStyle::new(&FONT_4X6, Rgb888::from(text_color.scale(200)));
    let _ = Text::with_baseline(letter, Point::new(28, 0), letter_style, Baseline::Top).draw(fb);

    // Horizontal progress bar: filled left-to-right based on elapsed time.
    let filled = if duration_ms == 0 {
        0
    } else {
        let elapsed = duration_ms.saturating_sub(remaining_ms) as u64;
        ((elapsed * PROGRESS_WIDTH as u64) / duration_ms as u64) as usize
    };

    let bar_color = base.scale(110);
    let track_color = base.scale(25);
    for i in 0..PROGRESS_WIDTH {
        let x = PROGRESS_X0 + i;
        let color = if i < filled { bar_color } else { track_color };
        for y in PROGRESS_Y0..PROGRESS_Y1 {
            fb.set_pixel(x, y, color);
        }
    }
}

fn base_color(phase: Phase) -> RgbColor {
    match phase {
        Phase::Work => RgbColor::new(255, 60, 0),
        Phase::Rest => RgbColor::new(0, 200, 90),
    }
}

fn format_mmss(mm: u16, ss: u8, buf: &mut [u8; 5]) -> &str {
    buf[0] = digit((mm / 10) as u8);
    buf[1] = digit((mm % 10) as u8);
    buf[2] = b':';
    buf[3] = digit(ss / 10);
    buf[4] = digit(ss % 10);
    // SAFETY: every byte we wrote is ASCII '0'..='9' or ':'.
    unsafe { core::str::from_utf8_unchecked(buf) }
}

const fn digit(n: u8) -> u8 {
    b'0' + (n % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_mmss_examples() {
        let mut buf = [0u8; 5];
        assert_eq!(format_mmss(0, 0, &mut buf), "00:00");
        assert_eq!(format_mmss(25, 5, &mut buf), "25:05");
        assert_eq!(format_mmss(99, 59, &mut buf), "99:59");
    }
}
