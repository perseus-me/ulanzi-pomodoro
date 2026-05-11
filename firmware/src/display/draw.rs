//! Bridge between the `embedded-graphics` ecosystem and our [`FrameBuffer`].
//!
//! Exposes `FrameBuffer` as a `DrawTarget<Color = Rgb888>` so that text, lines
//! and bitmaps from `embedded_graphics::primitives::*` can be rendered with
//! exactly the API the rest of the embedded-graphics crate offers.

use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{Rgb888, RgbColor as EgRgbColor},
};

use super::{FrameBuffer, MATRIX_HEIGHT, MATRIX_WIDTH, RgbColor};

impl From<Rgb888> for RgbColor {
    fn from(value: Rgb888) -> Self {
        Self {
            r: value.r(),
            g: value.g(),
            b: value.b(),
        }
    }
}

impl From<RgbColor> for Rgb888 {
    fn from(value: RgbColor) -> Self {
        Self::new(value.r, value.g, value.b)
    }
}

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        Size::new(MATRIX_WIDTH as u32, MATRIX_HEIGHT as u32)
    }
}

impl DrawTarget for FrameBuffer {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels.into_iter() {
            if point.x < 0 || point.y < 0 {
                continue;
            }
            let x = point.x as usize;
            let y = point.y as usize;
            self.set_pixel(x, y, color.into());
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.fill(color.into());
        Ok(())
    }
}
