//! Turns a captured frame into a small JPEG for previews and library thumbnails.

use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{ExtendedColorType, RgbImage, RgbaImage};

/// Shrinks an RGBA frame so it is at most `max_width` wide (never enlarges), keeping its proportions,
/// and encodes it as JPEG. Returns the bytes and the final size.
pub fn jpeg_from_rgba(rgba: &[u8], width: u32, height: u32, max_width: u32, quality: u8) -> Result<(Vec<u8>, u32, u32), String> {
    if width == 0 || height == 0 {
        return Err("empty frame".into());
    }
    let expected = width as usize * height as usize * 4;
    if rgba.len() < expected {
        return Err(format!("frame buffer too small: {} < {expected}", rgba.len()));
    }
    let image = RgbaImage::from_raw(width, height, rgba[..expected].to_vec()).ok_or("bad frame size")?;

    let target_w = width.min(max_width.max(1));
    let target_h = ((height as u64 * target_w as u64) / width as u64).max(1) as u32;
    let small = if target_w == width {
        image
    } else {
        // Triangle is much cheaper than Lanczos and plenty for a thumbnail.
        imageops::resize(&image, target_w, target_h, FilterType::Triangle)
    };

    // JPEG has no alpha.
    let rgb = RgbImage::from_fn(small.width(), small.height(), |x, y| {
        let p = small.get_pixel(x, y);
        image::Rgb([p[0], p[1], p[2]])
    });
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100))
        .encode(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok((out, rgb.width(), rgb.height()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Vec<u8> {
        (0..w * h).flat_map(|i| [(i % w * 255 / w) as u8, (i / w * 255 / h) as u8, 128, 255]).collect()
    }

    fn decode(bytes: &[u8]) -> image::DynamicImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).unwrap()
    }

    #[test]
    fn shrinks_to_the_requested_width_and_keeps_proportions() {
        let (bytes, w, h) = jpeg_from_rgba(&gradient(1920, 1080), 1920, 1080, 480, 80).unwrap();
        assert_eq!((w, h), (480, 270));
        let img = decode(&bytes);
        assert_eq!((img.width(), img.height()), (480, 270));
        assert!(bytes.len() < 40_000, "a thumbnail is small: {} bytes", bytes.len());
    }

    #[test]
    fn never_enlarges_a_small_frame() {
        let (_, w, h) = jpeg_from_rgba(&gradient(200, 100), 200, 100, 480, 80).unwrap();
        assert_eq!((w, h), (200, 100));
    }

    #[test]
    fn colours_survive_the_round_trip() {
        let red: Vec<u8> = (0..64 * 64).flat_map(|_| [220, 20, 20, 255]).collect();
        let (bytes, ..) = jpeg_from_rgba(&red, 64, 64, 64, 90).unwrap();
        let p = decode(&bytes).to_rgb8().get_pixel(32, 32).0;
        assert!(p[0] > 190 && p[1] < 60 && p[2] < 60, "still red: {p:?}");
    }

    #[test]
    fn rejects_bad_input_instead_of_panicking() {
        assert!(jpeg_from_rgba(&[], 0, 0, 100, 80).is_err());
        assert!(jpeg_from_rgba(&[0; 10], 100, 100, 50, 80).is_err());
    }

    #[test]
    fn very_tall_frames_keep_at_least_one_row() {
        let (_, w, h) = jpeg_from_rgba(&gradient(1000, 1), 1000, 1, 10, 80).unwrap();
        assert_eq!((w, h), (10, 1));
    }
}
