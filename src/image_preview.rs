use crate::store::ImageBlob;
use image::DynamicImage;
use image::codecs::bmp::BmpDecoder;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::io::Cursor;

pub fn thumbnail(image: &ImageBlob) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    if image.width.checked_mul(image.height)? > 20_000_000 {
        return None;
    }
    let decoder = BmpDecoder::new_without_file_header(Cursor::new(&image.dib)).ok()?;
    let decoded = DynamicImage::from_decoder(decoder).ok()?;
    let small = decoded.thumbnail(88, 60).into_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        small.as_raw(),
        small.width(),
        small.height(),
    ))
}
