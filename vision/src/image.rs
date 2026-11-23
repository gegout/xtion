use crate::DepthFrame;
use font8x8::{BASIC_FONTS, UnicodeFonts};
pub fn colour(mm: u16, far: u16) -> [u8; 3] {
    if mm == 0 {
        return [0; 3];
    }
    let t = (mm as f32 / far.max(1) as f32).min(1.);
    [
        (255. * (1. - t)) as u8,
        (255. * (1. - (2. * t - 1.).abs())) as u8,
        (255. * t) as u8,
    ]
}
pub fn rgb(f: &DepthFrame, width: usize, far: u16) -> (usize, usize, Vec<u8>) {
    let h = width * f.info.height / f.info.width;
    let mut data = vec![0; width * (h + 12) * 3];
    for y in 0..h {
        for x in 0..width {
            let p = f.pixels[y * f.info.height / h * f.info.width + x * f.info.width / width];
            let c = if f.info.metric {
                colour(p, far)
            } else {
                [0; 3]
            };
            data[(y * width + x) * 3..][..3].copy_from_slice(&c)
        }
    }
    let label = format!(
        "{:<pad$}{:.1}m",
        "near | black=invalid",
        far as f64 / 1000.,
        pad = (width / 8).saturating_sub(4)
    );
    for x in 0..width {
        let c = colour(
            1 + ((far.saturating_sub(1) as usize) * x / width) as u16,
            far,
        );
        for y in h..h + 3 {
            data[(y * width + x) * 3..][..3].copy_from_slice(&c)
        }
    }
    for (n, ch) in label.chars().enumerate() {
        if let Some(g) = BASIC_FONTS.get(ch) {
            for (row, bits) in g.iter().enumerate() {
                for col in 0..8 {
                    let x = n * 8 + col;
                    if x < width && bits & (1 << col) != 0 {
                        data[((h + 4 + row) * width + x) * 3..][..3].fill(255)
                    }
                }
            }
        }
    }
    (width, h + 12, data)
}
pub fn png(w: usize, h: usize, data: &[u8]) -> crate::source::Result<Vec<u8>> {
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(data)?;
    }
    Ok(out)
}
