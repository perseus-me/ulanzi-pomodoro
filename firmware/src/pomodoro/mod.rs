//! Pomodoro session state machine and preset model.

pub mod presets;
pub mod state;

pub use presets::Preset;
pub use state::{CompletedWork, Phase, PhaseTransition, Pomodoro, State};
