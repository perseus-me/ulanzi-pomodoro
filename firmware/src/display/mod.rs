//! WS2812B LED matrix driver for the Ulanzi TC001 32x8 display.
//!
//! The chain is wired in a row-major zig-zag: pixel (0,0) is the top-left
//! corner, pixels go left-to-right on even rows and right-to-left on odd rows.
//! The data line is GPIO32 and is driven via the ESP32 RMT peripheral.
//!
//! Ported from <https://github.com/cpa/ulanzi-tc001>.

#![allow(dead_code)]

use alloc::vec::Vec;
use esp_hal::{
    Blocking,
    gpio::Level,
    rmt::{Channel, Error as RmtError, PulseCode, Tx},
    time::Rate,
};

pub mod draw;

pub const MATRIX_WIDTH: usize = 32;
pub const MATRIX_HEIGHT: usize = 8;
pub const MATRIX_LED_COUNT: usize = MATRIX_WIDTH * MATRIX_HEIGHT;
pub const MATRIX_DATA_GPIO: u8 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
    };
    pub const RED: Self = Self { r: 255, g: 0, b: 0 };
    pub const GREEN: Self = Self { r: 0, g: 255, b: 0 };
    pub const BLUE: Self = Self { r: 0, g: 0, b: 255 };
    pub const YELLOW: Self = Self {
        r: 255,
        g: 200,
        b: 0,
    };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn scale(self, brightness: u8) -> Self {
        if brightness >= 255 {
            return self;
        }
        fn ch(value: u8, brightness: u8) -> u8 {
            ((value as u16 * brightness as u16 + 127) / 255) as u8
        }
        Self {
            r: ch(self.r, brightness),
            g: ch(self.g, brightness),
            b: ch(self.b, brightness),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelMapping {
    RowZigZag,
}

#[derive(Clone)]
pub struct FrameBuffer {
    pixels: [RgbColor; MATRIX_LED_COUNT],
    mapping: PixelMapping,
}

impl FrameBuffer {
    pub fn new(mapping: PixelMapping) -> Self {
        Self {
            pixels: [RgbColor::BLACK; MATRIX_LED_COUNT],
            mapping,
        }
    }

    pub fn clear(&mut self) {
        self.fill(RgbColor::BLACK);
    }

    pub fn fill(&mut self, color: RgbColor) {
        self.pixels.fill(color);
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: RgbColor) {
        if let Some(idx) = self.map_xy(x, y) {
            self.pixels[idx] = color;
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<RgbColor> {
        self.map_xy(x, y).map(|idx| self.pixels[idx])
    }

    pub fn pixels(&self) -> &[RgbColor; MATRIX_LED_COUNT] {
        &self.pixels
    }

    fn map_xy(&self, x: usize, y: usize) -> Option<usize> {
        if x >= MATRIX_WIDTH || y >= MATRIX_HEIGHT {
            return None;
        }
        match self.mapping {
            PixelMapping::RowZigZag => {
                let row_start = y * MATRIX_WIDTH;
                Some(if y % 2 == 0 {
                    row_start + x
                } else {
                    row_start + (MATRIX_WIDTH - 1 - x)
                })
            }
        }
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new(PixelMapping::RowZigZag)
    }
}

pub trait PixelSink {
    type Error;
    fn send(&mut self, pixels: &[RgbColor]) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum DisplayError<SinkError> {
    Sink(SinkError),
}

pub struct MatrixDisplay<SINK: PixelSink> {
    sink: SINK,
    buffer: FrameBuffer,
    brightness: u8,
    scaled: Vec<RgbColor>,
}

impl<SINK: PixelSink> MatrixDisplay<SINK> {
    pub fn new(sink: SINK, mapping: PixelMapping, brightness: u8) -> Self {
        Self {
            sink,
            buffer: FrameBuffer::new(mapping),
            brightness,
            scaled: Vec::with_capacity(MATRIX_LED_COUNT),
        }
    }

    pub fn dims(&self) -> (usize, usize) {
        (MATRIX_WIDTH, MATRIX_HEIGHT)
    }

    pub fn framebuffer(&self) -> &FrameBuffer {
        &self.buffer
    }

    pub fn framebuffer_mut(&mut self) -> &mut FrameBuffer {
        &mut self.buffer
    }

    pub fn set_brightness(&mut self, brightness: u8) {
        self.brightness = brightness;
    }

    pub fn brightness(&self) -> u8 {
        self.brightness
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: RgbColor) {
        self.buffer.set_pixel(x, y, color);
    }

    pub fn fill(&mut self, color: RgbColor) {
        self.buffer.fill(color);
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn flush(&mut self) -> Result<(), DisplayError<SINK::Error>> {
        if self.brightness >= 255 {
            return self
                .sink
                .send(self.buffer.pixels())
                .map_err(DisplayError::Sink);
        }

        self.scaled.clear();
        if self.scaled.capacity() < MATRIX_LED_COUNT {
            self.scaled
                .reserve(MATRIX_LED_COUNT - self.scaled.capacity());
        }
        for color in self.buffer.pixels().iter().copied() {
            self.scaled.push(color.scale(self.brightness));
        }

        self.sink.send(&self.scaled).map_err(DisplayError::Sink)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ws2812Timing {
    pub t0h_ns: u16,
    pub t0l_ns: u16,
    pub t1h_ns: u16,
    pub t1l_ns: u16,
    pub reset_ns: u32,
}

impl Ws2812Timing {
    pub const fn ws2812_800khz() -> Self {
        Self {
            t0h_ns: 350,
            t0l_ns: 900,
            t1h_ns: 700,
            t1l_ns: 600,
            reset_ns: 300_000,
        }
    }
}

pub struct Ws2812Sink<'a> {
    channel: Option<Channel<'a, Blocking, Tx>>,
    bit_zero: PulseCode,
    bit_one: PulseCode,
    reset: PulseCode,
    buffer: Vec<PulseCode>,
}

impl<'a> Ws2812Sink<'a> {
    pub fn new(
        channel: Channel<'a, Blocking, Tx>,
        source_clock: Rate,
        timing: Ws2812Timing,
    ) -> Self {
        let clk_hz = source_clock.as_hz();
        let bit_zero = PulseCode::new(
            Level::High,
            ticks(clk_hz, timing.t0h_ns as u32),
            Level::Low,
            ticks(clk_hz, timing.t0l_ns as u32),
        );
        let bit_one = PulseCode::new(
            Level::High,
            ticks(clk_hz, timing.t1h_ns as u32),
            Level::Low,
            ticks(clk_hz, timing.t1l_ns as u32),
        );
        let reset_ticks = ticks(clk_hz, timing.reset_ns);
        let reset = PulseCode::new(Level::Low, reset_ticks, Level::Low, reset_ticks);

        Self {
            channel: Some(channel),
            bit_zero,
            bit_one,
            reset,
            buffer: Vec::with_capacity(MATRIX_LED_COUNT * 24 + 8),
        }
    }

    fn encode_pixels(&mut self, pixels: &[RgbColor]) {
        let required = pixels.len().saturating_mul(24) + 2;
        if self.buffer.capacity() < required {
            self.buffer.reserve(required - self.buffer.capacity());
        }
        self.buffer.clear();

        for color in pixels {
            // WS2812B byte order is G, R, B.
            for &component in &[color.g, color.r, color.b] {
                for bit in (0..8).rev() {
                    let code = if (component >> bit) & 1 == 1 {
                        self.bit_one
                    } else {
                        self.bit_zero
                    };
                    self.buffer.push(code);
                }
            }
        }

        self.buffer.push(self.reset);
        self.buffer.push(PulseCode::end_marker());
    }
}

impl<'a> PixelSink for Ws2812Sink<'a> {
    type Error = RmtError;

    fn send(&mut self, pixels: &[RgbColor]) -> Result<(), Self::Error> {
        self.encode_pixels(pixels);
        let channel = self.channel.take().expect("RMT channel not available");
        let transaction = channel.transmit(&self.buffer)?;
        match transaction.wait() {
            Ok(returned) => {
                self.channel = Some(returned);
                Ok(())
            }
            Err((err, returned)) => {
                self.channel = Some(returned);
                Err(err)
            }
        }
    }
}

const fn ticks(clock_hz: u32, ns: u32) -> u16 {
    let numerator = clock_hz as u64 * ns as u64;
    let ticks = (numerator + 999_999_999) / 1_000_000_000;
    if ticks > PulseCode::MAX_LEN as u64 {
        PulseCode::MAX_LEN
    } else {
        ticks as u16
    }
}
