//! Frame model, payload encoding and image/GIF import.

use std::path::Path;

use image::AnimationDecoder;

pub const W: usize = 160;
pub const H: usize = 96;
pub const PAGE: usize = 4096;
pub const HEADER_LEN: usize = 256;
pub const MAX_FRAMES: usize = 255;
pub const DEFAULT_DELAY_MS: u32 = 40;

pub type Rgb = [u8; 3];

#[derive(Clone)]
pub struct Frame {
    pub px: Vec<Rgb>,
    pub delay_ms: u32,
}

impl Default for Frame {
    fn default() -> Self {
        Self::blank()
    }
}

impl Frame {
    pub fn blank() -> Self {
        Self { px: vec![[0, 0, 0]; W * H], delay_ms: DEFAULT_DELAY_MS }
    }

    /// Four colour quadrants with a yellow corner marker, used to verify the panel.
    pub fn testcard() -> Self {
        let mut frame = Self::blank();
        let (half_w, half_h) = (W / 2, H / 2);
        frame.rect(0, 0, half_w as isize - 1, half_h as isize - 1, [255, 0, 0]);
        frame.rect(half_w as isize, 0, W as isize - 1, half_h as isize - 1, [0, 255, 0]);
        frame.rect(0, half_h as isize, half_w as isize - 1, H as isize - 1, [0, 0, 255]);
        frame.rect(half_w as isize, half_h as isize, W as isize - 1, H as isize - 1, [255, 255, 255]);
        frame.rect(0, 0, 15, 3, [255, 255, 0]);
        for i in 0..30isize {
            frame.set(i, i, [255, 255, 0]);
            frame.set(i, i + 1, [255, 255, 0]);
        }
        frame
    }

    pub fn fill(&mut self, color: Rgb) {
        self.px.fill(color);
    }

    #[inline]
    pub fn set(&mut self, x: isize, y: isize, color: Rgb) {
        if x < 0 || y < 0 || x >= W as isize || y >= H as isize {
            return;
        }
        self.px[y as usize * W + x as usize] = color;
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Rgb {
        if x >= W || y >= H {
            return [0, 0, 0];
        }
        self.px[y * W + x]
    }

    /// Round brush; radius 0 paints a single pixel.
    pub fn brush(&mut self, cx: isize, cy: isize, radius: isize, color: Rgb) {
        if radius <= 0 {
            self.set(cx, cy, color);
            return;
        }
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.set(cx + dx, cy + dy, color);
                }
            }
        }
    }

    /// Filled rectangle, coordinates clamped to the panel.
    pub fn rect(&mut self, x0: isize, y0: isize, x1: isize, y1: isize, color: Rgb) {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, color);
            }
        }
    }

    pub fn outline(&mut self, x0: isize, y0: isize, x1: isize, y1: isize, color: Rgb) {
        for x in x0..=x1 {
            self.set(x, y0, color);
            self.set(x, y1, color);
        }
        for y in y0..=y1 {
            self.set(x0, y, color);
            self.set(x1, y, color);
        }
    }

    /// Horizontal gauge with an inset 1px frame, used by the live views.
    pub fn gauge(&mut self, x0: isize, y0: isize, x1: isize, fraction: f32, color: Rgb) {
        self.outline(x0, y0, x1, y0 + 5, [60, 64, 74]);
        let span = (x1 - x0 - 2) as f32;
        let filled = (span * fraction.clamp(0.0, 1.0)).round() as isize;
        if filled > 0 {
            self.rect(x0 + 1, y0 + 1, x0 + filled, y0 + 4, color);
        }
    }

    /// RGB565 little-endian, row major.
    pub fn to_rgb565_le(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(W * H * 2);
        for pixel in &self.px {
            let value = ((pixel[0] as u16 & 0xF8) << 8)
                | ((pixel[1] as u16 & 0xFC) << 3)
                | (pixel[2] as u16 >> 3);
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    /// RGBA bytes for the egui texture.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(W * H * 4);
        for pixel in &self.px {
            out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
        }
        out
    }
}

#[derive(Clone)]
pub struct Screen {
    pub frames: Vec<Frame>,
    pub selected: usize,
}

impl Default for Screen {
    fn default() -> Self {
        Self { frames: vec![Frame::blank()], selected: 0 }
    }
}

impl Screen {
    pub fn single(frame: Frame) -> Self {
        Self { frames: vec![frame], selected: 0 }
    }

    pub fn current(&self) -> &Frame {
        self.frames.get(self.selected).unwrap_or(&self.frames[0])
    }

    pub fn current_mut(&mut self) -> &mut Frame {
        let index = self.selected.min(self.frames.len().saturating_sub(1));
        &mut self.frames[index]
    }

    pub fn add_frame(&mut self) {
        let frame = self.current().clone();
        self.frames.push(frame);
        self.selected = self.frames.len() - 1;
    }

    pub fn duplicate(&mut self) {
        self.add_frame();
    }

    pub fn delete_frame(&mut self) {
        if self.frames.len() <= 1 {
            self.frames[0] = Frame::blank();
            self.selected = 0;
            return;
        }
        self.frames.remove(self.selected);
        self.selected = self.selected.min(self.frames.len() - 1);
    }

    pub fn total_ms(&self) -> u32 {
        self.frames.iter().map(|f| f.delay_ms).sum()
    }

    /// Build the payload the keyboard expects: 256-byte header + RGB565 frames.
    pub fn encode(&self) -> Vec<u8> {
        let count = self.frames.len().min(MAX_FRAMES).max(1);
        let mut payload = Vec::with_capacity(HEADER_LEN + count * W * H * 2 + PAGE);
        let mut header = [0xFFu8; HEADER_LEN];
        header[0] = count as u8;
        for (index, frame) in self.frames.iter().take(count).enumerate() {
            header[1 + index] = (frame.delay_ms.max(2) / 2).clamp(1, 255) as u8;
        }
        payload.extend_from_slice(&header);
        for frame in self.frames.iter().take(count) {
            payload.extend_from_slice(&frame.to_rgb565_le());
        }
        let pages = payload.len().div_ceil(PAGE);
        payload.resize(pages * PAGE, 0xFF);
        payload
    }

    pub fn page_count(&self) -> usize {
        self.encode().len() / PAGE
    }

    /// Load a still image or an animated GIF, replacing the timeline.
    pub fn import(&mut self, path: &Path) -> Result<usize, String> {
        let frames = decode_image(path)?;
        if frames.is_empty() {
            return Err("image contained no frames".into());
        }
        self.selected = 0;
        self.frames = frames;
        Ok(self.frames.len())
    }
}

fn decode_image(path: &Path) -> Result<Vec<Frame>, String> {
    let path_str = path.to_string_lossy().to_string();
    let reader = image::ImageReader::open(path)
        .map_err(|e| format!("cannot open {path_str}: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("cannot read {path_str}: {e}"))?;

    let format = reader.format();
    if format == Some(image::ImageFormat::Gif) {
        let decoder = image::codecs::gif::GifDecoder::new(
            std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?),
        )
        .map_err(|e| format!("cannot decode gif: {e}"))?;

        let mut frames: Vec<Frame> = Vec::new();
        for decoded in decoder.into_frames().collect_frames().map_err(|e| e.to_string())? {
            let (numer, denom) = decoded.delay().numer_denom_ms();
            let delay = if denom == 0 { DEFAULT_DELAY_MS } else { (numer / denom).max(20) };
            let rgba = decoded.into_buffer();
            let resized = image::imageops::resize(
                &image::DynamicImage::ImageRgba8(rgba).to_rgb8(),
                W as u32,
                H as u32,
                image::imageops::FilterType::Triangle,
            );
            frames.push(frame_from_image(&resized, delay));
        }
        if frames.len() > MAX_FRAMES {
            let step = frames.len() as f32 / MAX_FRAMES as f32;
            frames = (0..MAX_FRAMES)
                .map(|i| frames[(i as f32 * step) as usize].clone())
                .collect();
        }
        return Ok(frames);
    }

    let image = reader.decode().map_err(|e| format!("cannot decode {path_str}: {e}"))?;
    let resized = image::imageops::resize(
        &image.to_rgb8(),
        W as u32,
        H as u32,
        image::imageops::FilterType::Lanczos3,
    );
    Ok(vec![frame_from_image(&resized, DEFAULT_DELAY_MS)])
}

fn frame_from_image(image: &image::RgbImage, delay_ms: u32) -> Frame {
    let mut px = Vec::with_capacity(W * H);
    for pixel in image.pixels() {
        px.push([pixel[0], pixel[1], pixel[2]]);
    }
    Frame { px, delay_ms }
}

