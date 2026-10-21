//! A tiny RGB8 canvas: rectangles, 8x8-font text and deterministic PNG output.

use font8x8::legacy::BASIC_LEGACY;

pub(crate) type Rgb = [u8; 3];

pub(crate) struct Canvas {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, background: Rgb) -> Self {
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for _ in 0..width as usize * height as usize {
            data.extend_from_slice(&background);
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Set one pixel; coordinates outside the canvas are ignored.
    pub fn set(&mut self, x: u32, y: u32, color: Rgb) {
        if x < self.width && y < self.height {
            let i = (y as usize * self.width as usize + x as usize) * 3;
            self.data[i..i + 3].copy_from_slice(&color);
        }
    }

    pub fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Rgb) {
        for yy in y..(y.saturating_add(h)).min(self.height) {
            for xx in x..(x.saturating_add(w)).min(self.width) {
                self.set(xx, yy, color);
            }
        }
    }

    /// Width in pixels of `text` at integer `scale` (8 px per character).
    pub fn text_width(text: &str, scale: u32) -> u32 {
        text.chars().count() as u32 * 8 * scale
    }

    /// Draw `text` with its top-left corner at `(x, y)`. Characters outside
    /// printable ASCII draw as `?`.
    pub fn text(&mut self, x: u32, y: u32, text: &str, scale: u32, color: Rgb) {
        for (i, ch) in text.chars().enumerate() {
            let code = if ch.is_ascii() && !ch.is_ascii_control() {
                ch as usize
            } else {
                '?' as usize
            };
            let glyph = BASIC_LEGACY[code];
            let gx = x + i as u32 * 8 * scale;
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8u32 {
                    if bits >> col & 1 == 1 {
                        self.rect(
                            gx + col * scale,
                            y + row as u32 * scale,
                            scale,
                            scale,
                            color,
                        );
                    }
                }
            }
        }
    }

    /// `self` on top, `below` underneath. Both must have the same width.
    pub fn stacked(mut self, below: Canvas) -> Canvas {
        assert_eq!(self.width, below.width);
        self.height += below.height;
        self.data.extend_from_slice(&below.data);
        self
    }

    /// Encode as RGB8 PNG: fixed compression and filter, no ancillary chunks.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Balanced);
            encoder.set_filter(png::Filter::Sub);
            let mut writer = encoder.write_header().expect("header into a Vec");
            writer
                .write_image_data(&self.data)
                .expect("pixel data matches the header");
            writer.finish().expect("finish into a Vec");
        }
        out
    }
}
