//! Three-button input driver for the Ulanzi TC001.
//!
//! Each of the three front-panel buttons is wired between its GPIO and ground,
//! with the GPIO held high via an internal pull-up. A pressed button therefore
//! reads `Low`, a released button reads `High`.
//!
//! The reader:
//!   * debounces samples with a 20 ms stable-state window,
//!   * emits a `Press` event on every clean release that happened *before*
//!     the long-press threshold (so a "click" only fires once the user lets
//!     go, which matches what most physical UIs do),
//!   * emits a `LongPress` event as soon as a button has been held for 700 ms,
//!   * emits repeated `Repeat` events every 150 ms while a button is still
//!     held after the long press — handy for "+/-" style menus.
//!
//! All time-keeping is done in milliseconds against a free-running monotonic
//! counter supplied by the caller, so the driver is independent of any
//! particular HAL clock.

use alloc::vec::Vec;
use esp_hal::gpio::Input;

const DEBOUNCE_MS: u32 = 20;
const LONG_PRESS_MS: u32 = 700;
const REPEAT_PERIOD_MS: u32 = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonId {
    Left,
    Select,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonEvent {
    /// Short click (button released without crossing the long-press threshold).
    Press(ButtonId),
    /// Held for at least `LONG_PRESS_MS`. Fires exactly once per hold.
    LongPress(ButtonId),
    /// Auto-repeat tick. Fires every `REPEAT_PERIOD_MS` *after* the initial
    /// `LongPress`, useful for adjusting numeric values.
    Repeat(ButtonId),
}

struct Channel<'a> {
    id: ButtonId,
    pin: Input<'a>,
    /// Last raw GPIO sample (true = pressed).
    raw_pressed: bool,
    /// Last *debounced* state (true = pressed).
    stable_pressed: bool,
    /// Timestamp of the last raw sample that matched the stable state, in ms.
    stable_since_ms: u64,
    /// When the button transitioned from released to pressed, in ms.
    pressed_at_ms: u64,
    /// Whether we've already emitted a `LongPress` for the current hold.
    long_press_emitted: bool,
    /// Time of the last `Repeat` event in the current hold.
    last_repeat_ms: u64,
}

impl<'a> Channel<'a> {
    fn new(id: ButtonId, pin: Input<'a>) -> Self {
        // Always boot in the "released" state, regardless of what the pin
        // actually reads. If a button happens to be held while the firmware
        // starts, the normal debouncing path will pick that up after
        // `DEBOUNCE_MS` and produce a real Press transition — without this
        // we would otherwise emit a phantom LongPress on the first frame.
        Self {
            id,
            pin,
            raw_pressed: false,
            stable_pressed: false,
            stable_since_ms: 0,
            pressed_at_ms: 0,
            long_press_emitted: false,
            last_repeat_ms: 0,
        }
    }

    fn poll(&mut self, now_ms: u64, out: &mut Vec<ButtonEvent>) {
        let raw = self.pin.is_low();

        // Track when the raw signal last changed. We only believe a new
        // stable state once `DEBOUNCE_MS` have passed without a flip.
        if raw != self.raw_pressed {
            self.raw_pressed = raw;
            self.stable_since_ms = now_ms;
        }

        let elapsed_stable = now_ms.saturating_sub(self.stable_since_ms);
        if raw != self.stable_pressed && elapsed_stable >= DEBOUNCE_MS as u64 {
            // The signal has been steady long enough — accept the new state.
            self.stable_pressed = raw;
            if self.stable_pressed {
                // Released -> pressed.
                self.pressed_at_ms = now_ms;
                self.long_press_emitted = false;
                self.last_repeat_ms = now_ms;
            } else {
                // Pressed -> released. Emit a short-press only if we never
                // crossed the long-press threshold, otherwise the long press
                // is already considered handled.
                let held = now_ms.saturating_sub(self.pressed_at_ms);
                if !self.long_press_emitted && held < LONG_PRESS_MS as u64 {
                    out.push(ButtonEvent::Press(self.id));
                }
            }
        }

        // While the button is held, generate exactly one `LongPress` and then
        // a periodic `Repeat` stream.
        if self.stable_pressed {
            let held = now_ms.saturating_sub(self.pressed_at_ms);
            if !self.long_press_emitted && held >= LONG_PRESS_MS as u64 {
                self.long_press_emitted = true;
                self.last_repeat_ms = now_ms;
                out.push(ButtonEvent::LongPress(self.id));
            } else if self.long_press_emitted
                && now_ms.saturating_sub(self.last_repeat_ms) >= REPEAT_PERIOD_MS as u64
            {
                self.last_repeat_ms = now_ms;
                out.push(ButtonEvent::Repeat(self.id));
            }
        }
    }
}

pub struct Buttons<'a> {
    channels: [Channel<'a>; 3],
}

impl<'a> Buttons<'a> {
    /// Wrap three already-configured `Input` pins.
    ///
    /// The caller is expected to set them up as active-low inputs with
    /// internal pull-ups, typically:
    ///
    /// ```ignore
    /// let cfg = InputConfig::default().with_pull(Pull::Up);
    /// let left   = Input::new(peripherals.GPIO26, cfg);
    /// let select = Input::new(peripherals.GPIO27, cfg);
    /// let right  = Input::new(peripherals.GPIO14, cfg);
    /// let buttons = Buttons::new(left, select, right);
    /// ```
    pub fn new(left: Input<'a>, select: Input<'a>, right: Input<'a>) -> Self {
        Self {
            channels: [
                Channel::new(ButtonId::Left, left),
                Channel::new(ButtonId::Select, select),
                Channel::new(ButtonId::Right, right),
            ],
        }
    }

    /// Sample all buttons and append any newly produced events to `out`.
    ///
    /// `now_ms` is expected to be a monotonic millisecond counter.
    pub fn poll(&mut self, now_ms: u64, out: &mut Vec<ButtonEvent>) {
        for ch in self.channels.iter_mut() {
            ch.poll(now_ms, out);
        }
    }
}
