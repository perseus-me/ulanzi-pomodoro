//! Settings menu, reached by long-pressing SELECT in the Idle screen.
//!
//! Seven items, one per scroll position:
//!
//! | idx | label | range          | step |
//! |-----|-------|----------------|------|
//! |  0  | BRT   | 8..=255        | 16   |
//! |  1  | P1W   | 1..=240 min    | 5    |
//! |  2  | P1R   | 1..=240 min    | 5    |
//! |  3  | P2W   | 1..=240 min    | 5    |
//! |  4  | P2R   | 1..=240 min    | 5    |
//! |  5  | P3W   | 1..=240 min    | 5    |
//! |  6  | P3R   | 1..=240 min    | 5    |
//!
//! Buttons:
//!   * **Not editing:**
//!     - Press LEFT / RIGHT — move to previous / next item
//!     - Press SELECT — enter edit mode for the current item
//!     - Long-press SELECT — exit menu and save settings
//!   * **Editing:**
//!     - Press / Repeat LEFT — decrement
//!     - Press / Repeat RIGHT — increment
//!     - Press SELECT — leave edit mode (stays in menu)
//!     - Long-press SELECT — exit menu and save settings

use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_4X6},
    pixelcolor::Rgb888,
    prelude::Point,
    text::{Baseline, Text},
};

use crate::{
    display::{FrameBuffer, MATRIX_WIDTH, RgbColor},
    input::{ButtonEvent, ButtonId},
    pomodoro::presets::ADJUST_STEP_MIN,
    ui::{RenderContext, UiAction},
};

const ITEM_COUNT: u8 = 7;
const BRIGHTNESS_STEP: i16 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuState {
    pub item: u8,
    pub editing: bool,
}

impl MenuState {
    pub const fn new() -> Self {
        Self {
            item: 0,
            editing: false,
        }
    }
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn handle(
    ev: ButtonEvent,
    state: &mut MenuState,
    _ctx: &RenderContext<'_>,
) -> Option<UiAction> {
    if !state.editing {
        match ev {
            ButtonEvent::Press(ButtonId::Left) => {
                state.item = (state.item + ITEM_COUNT - 1) % ITEM_COUNT;
                None
            }
            ButtonEvent::Press(ButtonId::Right) => {
                state.item = (state.item + 1) % ITEM_COUNT;
                None
            }
            ButtonEvent::Press(ButtonId::Select) => {
                state.editing = true;
                None
            }
            ButtonEvent::LongPress(ButtonId::Select) => Some(UiAction::ExitMenu),
            _ => None,
        }
    } else {
        match ev {
            ButtonEvent::Press(ButtonId::Left) | ButtonEvent::Repeat(ButtonId::Left) => {
                adjustment(state.item, -1)
            }
            ButtonEvent::Press(ButtonId::Right) | ButtonEvent::Repeat(ButtonId::Right) => {
                adjustment(state.item, 1)
            }
            ButtonEvent::Press(ButtonId::Select) => {
                state.editing = false;
                None
            }
            ButtonEvent::LongPress(ButtonId::Select) => {
                state.editing = false;
                Some(UiAction::ExitMenu)
            }
            _ => None,
        }
    }
}

fn adjustment(item: u8, sign: i16) -> Option<UiAction> {
    let step = sign.signum() * ADJUST_STEP_MIN as i16;
    match item {
        0 => Some(UiAction::AdjustBrightness(sign.signum() * BRIGHTNESS_STEP)),
        1 => Some(UiAction::AdjustWork {
            preset_idx: 0,
            delta_min: step,
        }),
        2 => Some(UiAction::AdjustRest {
            preset_idx: 0,
            delta_min: step,
        }),
        3 => Some(UiAction::AdjustWork {
            preset_idx: 1,
            delta_min: step,
        }),
        4 => Some(UiAction::AdjustRest {
            preset_idx: 1,
            delta_min: step,
        }),
        5 => Some(UiAction::AdjustWork {
            preset_idx: 2,
            delta_min: step,
        }),
        6 => Some(UiAction::AdjustRest {
            preset_idx: 2,
            delta_min: step,
        }),
        _ => None,
    }
}

pub fn render(fb: &mut FrameBuffer, state: &MenuState, ctx: &RenderContext<'_>) {
    // Top row dots: one per item, current item is bright. Helps the user
    // count where they are without having to keep scrolling.
    for i in 0..ITEM_COUNT as usize {
        let x = i * 4 + 1;
        if x >= MATRIX_WIDTH {
            break;
        }
        let color = if i == state.item as usize {
            RgbColor::new(255, 200, 0)
        } else {
            RgbColor::new(40, 40, 60)
        };
        fb.set_pixel(x, 0, color);
    }

    // Label + value on the bottom 6 rows.
    let (label, value) = current_value(state.item, ctx);

    let label_style = MonoTextStyle::new(&FONT_4X6, Rgb888::new(180, 180, 180));
    let _ = Text::with_baseline(label, Point::new(0, 2), label_style, Baseline::Top).draw(fb);

    let blink_on = (ctx.now_ms / 350) % 2 == 0;
    let value_color = if state.editing && !blink_on {
        Rgb888::new(80, 60, 0)
    } else {
        Rgb888::new(255, 200, 0)
    };
    let value_style = MonoTextStyle::new(&FONT_4X6, value_color);
    let mut buf = [0u8; 3];
    let value_text = format_u16(value, &mut buf);
    // Value is right-aligned to keep the layout stable as digits drop off.
    let value_x = 31i32 - 4 * value_text.len() as i32;
    let _ = Text::with_baseline(value_text, Point::new(value_x, 2), value_style, Baseline::Top)
        .draw(fb);
}

fn current_value(item: u8, ctx: &RenderContext<'_>) -> (&'static str, u16) {
    match item {
        0 => ("BRT", ctx.settings.brightness as u16),
        1 => ("P1W", ctx.settings.presets[0].work_min),
        2 => ("P1R", ctx.settings.presets[0].rest_min),
        3 => ("P2W", ctx.settings.presets[1].work_min),
        4 => ("P2R", ctx.settings.presets[1].rest_min),
        5 => ("P3W", ctx.settings.presets[2].work_min),
        6 => ("P3R", ctx.settings.presets[2].rest_min),
        _ => ("???", 0),
    }
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
