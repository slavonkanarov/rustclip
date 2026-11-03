use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use std::io::Cursor;

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits
}

fn decode_pixels(mut decoder: impl ImageDecoder) -> Result<DynamicImage, String> {
    let mut limits = limits();
    let (width, height) = decoder.dimensions();
    limits
        .check_dimensions(width, height)
        .map_err(|e| e.to_string())?;
    let rgba_bytes = u64::from(width) * u64::from(height) * 4;
    limits
        .reserve(decoder.total_bytes().max(rgba_bytes))
        .map_err(|e| e.to_string())?;
    decoder.set_limits(limits).map_err(|e| e.to_string())?;
    DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())
}

pub fn decode(bytes: &[u8], dib: bool) -> Result<DynamicImage, String> {
    crate::diagnostics::recover("Image decoding", || {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("Image exceeds the 32 MiB limit".into());
        }
        if dib {
            let decoder =
                image::codecs::bmp::BmpDecoder::new_without_file_header(Cursor::new(bytes))
                    .map_err(|e| e.to_string())?;
            decode_pixels(decoder)
        } else {
            let mut reader = ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
            reader.limits(limits());
            decode_pixels(reader.into_decoder().map_err(|e| e.to_string())?)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_images_are_rejected_and_next_image_still_decodes() {
        assert!(decode(b"broken", false).is_err());
        assert!(decode(b"broken", true).is_err());
        assert!(decode(include_bytes!("../assets/icon.png"), false).is_ok());
    }

    #[test]
    fn oversized_dib_dimensions_are_rejected_before_pixel_allocation() {
        let mut dib = vec![0; 40];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&100_000i32.to_le_bytes());
        dib[8..12].copy_from_slice(&100_000i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        assert!(decode(&dib, true).is_err());
    }

    #[test]
    fn clipboard_dib_decodes_without_a_bmp_file_header() {
        let mut dib = vec![0; 40];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&1i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        dib.extend_from_slice(&[0, 0, 255, 0]);
        let decoded = decode(&dib, true).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (1, 1));
        assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn decoded_pixel_allocation_limit_is_enforced() {
        let mut dib = vec![0; 40];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&8192i32.to_le_bytes());
        dib[8..12].copy_from_slice(&8192i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        let error = decode(&dib, true).unwrap_err();
        assert!(error.to_lowercase().contains("memory"), "{error}");
    }
}
