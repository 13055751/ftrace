//! Animated GIF export (gif crate, MIT/Apache-2.0) — the "drawing" animation:
//! epicycles rotate and the trace grows contour by contour.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use image::RgbImage;

pub fn write_gif(path: &Path, frames: &[RgbImage], delay_cs: u16) -> Result<(), Box<dyn std::error::Error>> {
    let first = frames
        .first()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no frames"))?;
    let (w, h) = first.dimensions();
    let file = File::create(path)?;
    let mut encoder = gif::Encoder::new(BufWriter::new(file), w as u16, h as u16, &[])?;
    encoder.set_repeat(gif::Repeat::Infinite)?;
    for img in frames {
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for p in img.pixels() {
            data.push(p.0[0]);
            data.push(p.0[1]);
            data.push(p.0[2]);
        }
        let mut fr = gif::Frame::from_rgb_speed(w as u16, h as u16, &data, 10);
        fr.delay = delay_cs.max(1);
        encoder.write_frame(&fr)?;
    }
    Ok(())
}