//! Three user-configurable Pomodoro presets, one per front-panel button.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub work_min: u16,
    pub rest_min: u16,
}

impl Preset {
    pub const fn new(work_min: u16, rest_min: u16) -> Self {
        Self { work_min, rest_min }
    }
}

/// Defaults: 25/5 (classic), 50/10 (deep work), 90/20 (full session).
///
/// These match the three "slots" the user picks via the LEFT / SELECT / RIGHT
/// buttons when the clock is idle.
pub const DEFAULT_PRESETS: [Preset; 3] = [
    Preset::new(25, 5),
    Preset::new(50, 10),
    Preset::new(90, 20),
];

pub const MIN_MINUTES: u16 = 1;
pub const MAX_MINUTES: u16 = 240;
pub const ADJUST_STEP_MIN: u16 = 5;
