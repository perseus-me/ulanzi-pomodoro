//! Pomodoro FSM.
//!
//! ```text
//!   Idle ── start(N) ───────────► Running{Work}
//!   Running{Work} ── tick (t=0) ─► Running{Rest}   (and pushes CompletedWork)
//!   Running{Rest} ── tick (t=0) ─► Idle
//!   Running{*} ── toggle_pause ──► Paused{*}
//!   Paused{*}  ── toggle_pause ──► Running{*}
//!   *          ── stop ─────────► Idle  (no stats credited)
//! ```
//!
//! Time is tracked in monotonic milliseconds supplied by the caller, which
//! keeps the state machine independent of how the rest of the firmware
//! sources its clock.

use super::presets::{DEFAULT_PRESETS, Preset};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Work,
    Rest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Running {
        preset_idx: u8,
        phase: Phase,
        /// Monotonic ms at which this phase started.
        started_ms: u64,
        /// Phase length in ms (cached so callers can compute progress).
        duration_ms: u32,
    },
    Paused {
        preset_idx: u8,
        phase: Phase,
        /// Remaining milliseconds when the user paused.
        remaining_ms: u32,
        /// Original phase length, retained so the progress bar still makes
        /// sense after a resume.
        duration_ms: u32,
    },
}

/// Description of a work phase that has just completed, ready to be appended
/// to long-term statistics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletedWork {
    pub preset_idx: u8,
    pub minutes: u16,
}

/// Emitted by [`Pomodoro::tick`] whenever the FSM transitions on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseTransition {
    /// Work finished, the timer just rolled over into rest.
    WorkFinished,
    /// Rest finished, the timer is now Idle.
    RestFinished,
}

pub struct Pomodoro {
    state: State,
    presets: [Preset; 3],
    pending_completion: Option<CompletedWork>,
}

impl Pomodoro {
    pub fn new(presets: [Preset; 3]) -> Self {
        Self {
            state: State::Idle,
            presets,
            pending_completion: None,
        }
    }

    pub fn with_default_presets() -> Self {
        Self::new(DEFAULT_PRESETS)
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn presets(&self) -> &[Preset; 3] {
        &self.presets
    }

    pub fn presets_mut(&mut self) -> &mut [Preset; 3] {
        &mut self.presets
    }

    /// Start a session with the given preset. Returns `true` if the FSM was
    /// idle and the request was accepted, `false` otherwise (the caller can
    /// either ignore the press or treat it as "restart" — currently we keep
    /// it as a no-op so accidental clicks during a session don't lose state).
    pub fn start(&mut self, preset_idx: u8, now_ms: u64) -> bool {
        if !matches!(self.state, State::Idle) {
            return false;
        }
        let preset = match self.presets.get(preset_idx as usize) {
            Some(p) => *p,
            None => return false,
        };
        self.state = State::Running {
            preset_idx,
            phase: Phase::Work,
            started_ms: now_ms,
            duration_ms: minutes_to_ms(preset.work_min),
        };
        true
    }

    /// Pause a running session or resume a paused one. No-op when idle.
    pub fn toggle_pause(&mut self, now_ms: u64) {
        match self.state {
            State::Running {
                preset_idx,
                phase,
                started_ms,
                duration_ms,
            } => {
                let elapsed = now_ms.saturating_sub(started_ms).min(duration_ms as u64) as u32;
                let remaining_ms = duration_ms.saturating_sub(elapsed);
                self.state = State::Paused {
                    preset_idx,
                    phase,
                    remaining_ms,
                    duration_ms,
                };
            }
            State::Paused {
                preset_idx,
                phase,
                remaining_ms,
                duration_ms,
            } => {
                let started_ms = now_ms.saturating_sub((duration_ms - remaining_ms) as u64);
                self.state = State::Running {
                    preset_idx,
                    phase,
                    started_ms,
                    duration_ms,
                };
            }
            State::Idle => {}
        }
    }

    /// Force-return to idle without crediting any stats.
    pub fn stop(&mut self) {
        self.state = State::Idle;
        self.pending_completion = None;
    }

    /// Skip the current phase. Used to bail out of the rest phase early; for
    /// the work phase this is equivalent to [`Pomodoro::stop`] (no credit).
    pub fn skip(&mut self, _now_ms: u64) -> Option<PhaseTransition> {
        match self.state {
            State::Running { phase, .. } | State::Paused { phase, .. } => {
                if matches!(phase, Phase::Rest) {
                    self.state = State::Idle;
                    Some(PhaseTransition::RestFinished)
                } else {
                    self.stop();
                    None
                }
            }
            State::Idle => None,
        }
    }

    /// Advance the FSM. Returns a [`PhaseTransition`] when the timer rolls
    /// over so the UI can flash and the buzzer can beep.
    pub fn tick(&mut self, now_ms: u64) -> Option<PhaseTransition> {
        let State::Running {
            preset_idx,
            phase,
            started_ms,
            duration_ms,
        } = self.state
        else {
            return None;
        };

        let elapsed = now_ms.saturating_sub(started_ms);
        if elapsed < duration_ms as u64 {
            return None;
        }

        // Phase is over.
        match phase {
            Phase::Work => {
                let preset = self.presets[preset_idx as usize];
                self.pending_completion = Some(CompletedWork {
                    preset_idx,
                    minutes: preset.work_min,
                });
                // Schedule the rest phase, accounting for any overshoot so a
                // delayed tick doesn't make the rest interval longer than it
                // should be.
                let overshoot = elapsed - duration_ms as u64;
                let rest_ms = minutes_to_ms(preset.rest_min);
                let rest_started_ms = now_ms.saturating_sub(overshoot);
                self.state = State::Running {
                    preset_idx,
                    phase: Phase::Rest,
                    started_ms: rest_started_ms,
                    duration_ms: rest_ms,
                };
                Some(PhaseTransition::WorkFinished)
            }
            Phase::Rest => {
                self.state = State::Idle;
                Some(PhaseTransition::RestFinished)
            }
        }
    }

    pub fn take_completed_work(&mut self) -> Option<CompletedWork> {
        self.pending_completion.take()
    }

    /// Remaining milliseconds in the current phase, if any.
    pub fn remaining_ms(&self, now_ms: u64) -> Option<u32> {
        match self.state {
            State::Running {
                started_ms,
                duration_ms,
                ..
            } => {
                let elapsed = now_ms.saturating_sub(started_ms).min(duration_ms as u64) as u32;
                Some(duration_ms.saturating_sub(elapsed))
            }
            State::Paused { remaining_ms, .. } => Some(remaining_ms),
            State::Idle => None,
        }
    }

    pub fn duration_ms(&self) -> Option<u32> {
        match self.state {
            State::Running { duration_ms, .. } | State::Paused { duration_ms, .. } => {
                Some(duration_ms)
            }
            State::Idle => None,
        }
    }

    pub fn phase(&self) -> Option<Phase> {
        match self.state {
            State::Running { phase, .. } | State::Paused { phase, .. } => Some(phase),
            State::Idle => None,
        }
    }

    pub fn is_paused(&self) -> bool {
        matches!(self.state, State::Paused { .. })
    }
}

const fn minutes_to_ms(minutes: u16) -> u32 {
    (minutes as u32) * 60_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_cycle() {
        let mut p = Pomodoro::new([Preset::new(1, 1), Preset::new(2, 2), Preset::new(3, 3)]);
        let start = 0u64;
        assert!(p.start(0, start));
        // Half-way through work: nothing happens.
        assert!(p.tick(30_000).is_none());
        // Work boundary triggers transition + credit.
        assert_eq!(p.tick(60_000), Some(PhaseTransition::WorkFinished));
        let credit = p.take_completed_work().unwrap();
        assert_eq!(credit.minutes, 1);
        assert_eq!(p.phase(), Some(Phase::Rest));
        // End of rest goes idle.
        assert_eq!(p.tick(120_000), Some(PhaseTransition::RestFinished));
        assert!(matches!(p.state(), State::Idle));
    }

    #[test]
    fn pause_resume_preserves_remaining() {
        let mut p = Pomodoro::new(DEFAULT_PRESETS);
        p.start(0, 0);
        p.toggle_pause(10_000);
        assert!(matches!(p.state(), State::Paused { .. }));
        // 20 seconds pass while paused.
        p.toggle_pause(30_000);
        // Remaining should still be 25min - 10s = 24:50.
        let remaining = p.remaining_ms(30_000).unwrap();
        assert_eq!(remaining, 25 * 60_000 - 10_000);
    }

    #[test]
    fn abort_does_not_credit_stats() {
        let mut p = Pomodoro::new(DEFAULT_PRESETS);
        p.start(0, 0);
        p.stop();
        assert!(p.take_completed_work().is_none());
        assert!(matches!(p.state(), State::Idle));
    }
}
