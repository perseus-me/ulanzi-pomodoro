//! Audible / visual notifications.
//!
//! The TC001 uses a passive piezo buzzer on GPIO15. The bootloader leaves the
//! pin floating which sometimes produces a continuous low-frequency whine, so
//! we explicitly drive it `Low` whenever we aren't beeping.
//!
//! Beeping is done by bit-banging the GPIO at ~2 kHz; we don't bother with
//! the LEDC peripheral because beeps fire only on phase transitions (every
//! 25-90 minutes), so the simplicity wins over the few hundred ms of blocking.

use esp_hal::{
    gpio::Output,
    time::{Duration, Instant},
};

const BEEP_FREQ_HZ: u32 = 2_000;

pub struct Notifier<'a> {
    buzzer: Output<'a>,
}

impl<'a> Notifier<'a> {
    pub fn new(buzzer: Output<'a>) -> Self {
        let mut me = Self { buzzer };
        me.buzzer.set_low();
        me
    }

    /// Single beep of approximately `duration_ms` at ~2 kHz.
    pub fn beep(&mut self, duration_ms: u32) {
        self.tone(BEEP_FREQ_HZ, duration_ms);
    }

    /// "Phase finished" pattern: three short pips. Total duration is well
    /// under one second.
    pub fn finish(&mut self) {
        for _ in 0..3 {
            self.beep(120);
            wait_ms(80);
        }
    }

    /// Single click on a successful button-driven action.
    pub fn click(&mut self) {
        self.tone(BEEP_FREQ_HZ * 2, 12);
    }

    fn tone(&mut self, freq_hz: u32, duration_ms: u32) {
        if freq_hz == 0 || duration_ms == 0 {
            return;
        }
        let half_period_us = 1_000_000 / (freq_hz * 2);
        let total = Instant::now();
        while total.elapsed() < Duration::from_millis(duration_ms as u64) {
            self.buzzer.set_high();
            spin_us(half_period_us);
            self.buzzer.set_low();
            spin_us(half_period_us);
        }
        self.buzzer.set_low();
    }
}

fn spin_us(us: u32) {
    let start = Instant::now();
    let target = Duration::from_micros(us as u64);
    while start.elapsed() < target {
        core::hint::spin_loop();
    }
}

fn wait_ms(ms: u32) {
    let start = Instant::now();
    let target = Duration::from_millis(ms as u64);
    while start.elapsed() < target {
        core::hint::spin_loop();
    }
}
