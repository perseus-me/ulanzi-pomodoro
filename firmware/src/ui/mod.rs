//! On-display UI: routes button events to action requests and renders the
//! current screen onto the matrix framebuffer.
//!
//! The UI is intentionally side-effect-free: it never mutates the Pomodoro
//! FSM (or any settings) directly — instead it returns [`UiAction`] values
//! that `main` then applies. That keeps screens easy to test in isolation
//! and makes the data flow obvious.

pub mod idle;
pub mod menu;
pub mod notify;
pub mod running;

use crate::{
    display::FrameBuffer,
    input::ButtonEvent,
    pomodoro::{Pomodoro, State},
    storage::{Settings, Stats},
};

/// Side-effecting requests the UI wants `main` to apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    StartPreset(u8),
    TogglePause,
    StopSession,
    SkipRest,

    /// Open the settings menu.
    EnterMenu,
    /// Exit the menu, persisting any in-memory settings changes.
    ExitMenu,

    /// Adjust the work duration of preset `preset_idx` by `delta_min`.
    AdjustWork {
        preset_idx: u8,
        delta_min: i16,
    },
    /// Adjust the rest duration of preset `preset_idx` by `delta_min`.
    AdjustRest {
        preset_idx: u8,
        delta_min: i16,
    },
    /// Adjust the global LED brightness by `delta` (positive or negative).
    AdjustBrightness(i16),
}

/// All read-only state a screen might want to look at while rendering.
pub struct RenderContext<'a> {
    pub pomodoro: &'a Pomodoro,
    pub settings: &'a Settings,
    pub stats: &'a Stats,
    /// Today's calendar date as `YYYYMMDD`, or `None` if the clock has never
    /// been anchored.
    pub today: Option<u32>,
    /// Whether we managed to anchor the clock from NTP. Drives the tiny
    /// indicator dot in the corner.
    pub time_synced: bool,
    /// Free-running monotonic milliseconds since boot. Used for animations
    /// (blinking, etc.).
    pub now_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Idle,
    Running,
    Menu,
}

pub struct Ui {
    screen: Screen,
    menu_state: menu::MenuState,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            screen: Screen::Idle,
            menu_state: menu::MenuState::new(),
        }
    }

    /// Should be called whenever the Pomodoro FSM transitions on its own
    /// (e.g. when a phase boundary is reached) so the UI follows along.
    /// Never overrides the Menu screen.
    pub fn sync_to_state(&mut self, state: State) {
        if matches!(self.screen, Screen::Menu) {
            return;
        }
        self.screen = match state {
            State::Idle => Screen::Idle,
            State::Running { .. } | State::Paused { .. } => Screen::Running,
        };
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn handle(&mut self, ev: ButtonEvent, ctx: &RenderContext<'_>) -> Option<UiAction> {
        let action = match self.screen {
            Screen::Idle => idle::handle(ev, ctx),
            Screen::Running => running::handle(ev, ctx),
            Screen::Menu => menu::handle(ev, &mut self.menu_state, ctx),
        };
        if let Some(act) = action {
            self.preview(act);
        }
        action
    }

    /// Anticipate the screen transition that an action will cause so the next
    /// render uses the correct screen even before `sync_to_state` runs.
    fn preview(&mut self, action: UiAction) {
        match action {
            UiAction::StartPreset(_) => self.screen = Screen::Running,
            UiAction::StopSession => self.screen = Screen::Idle,
            UiAction::EnterMenu => {
                self.menu_state = menu::MenuState::new();
                self.screen = Screen::Menu;
            }
            UiAction::ExitMenu => self.screen = Screen::Idle,
            UiAction::SkipRest
            | UiAction::TogglePause
            | UiAction::AdjustWork { .. }
            | UiAction::AdjustRest { .. }
            | UiAction::AdjustBrightness(_) => {}
        }
    }

    pub fn render(&self, fb: &mut FrameBuffer, ctx: &RenderContext<'_>) {
        fb.clear();
        match self.screen {
            Screen::Idle => idle::render(fb, ctx),
            Screen::Running => running::render(fb, ctx),
            Screen::Menu => menu::render(fb, &self.menu_state, ctx),
        }
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}
