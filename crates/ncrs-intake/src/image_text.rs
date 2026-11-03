//! Photos and scans stored as image files: they have no text of their own, so
//! the file is handed on as one page image for the image sources (OCR).

use std::fs::File;
use std::io::{Cursor, Read as _};
use std::path::Path;

use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageDecoder as _, ImageReader, Limits};

use crate::page_trim::trim_to_content;
use crate::text::{Extracted, PageImage, TextError, TextSource, MAX_SCAN_SIDE_PIXELS};

const EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "tif", "tiff"];
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SIDE_PIXELS: u32 = 0x4000;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default)]
pub struct ImageFileText;

fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

fn decode_page(bytes: Vec<u8>) -> Option<PageImage> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE_PIXELS);
    limits.max_image_height = Some(MAX_SIDE_PIXELS);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(upright) = orientation {
        picture.apply_orientation(upright);
    }
    let mut grey = picture.into_luma8();
    let longer_side = f64::from(grey.width().max(grey.height()));
    let cap = f64::from(MAX_SCAN_SIDE_PIXELS);
    if longer_side > cap {
        let scaled = |side: u32| {
            // Rounded and at most the cap, so the value fits a u32.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pixels = (f64::from(side) * cap / longer_side).round() as u32;
            pixels.max(1)
        };
        grey = imageops::resize(
            &grey,
            scaled(grey.width()),
            scaled(grey.height()),
            FilterType::Triangle,
        );
    }
    Some(trim_to_content(PageImage {
        width: grey.width(),
        height: grey.height(),
        pixels: grey.into_raw(),
    }))
}

impl TextSource for ImageFileText {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        if !is_image_extension(path) {
            return Ok(Extracted::NotApplicable);
        }
        let unreadable = |err: std::io::Error| TextError::Unreadable {
            path: path.to_path_buf(),
            reason: err.to_string(),
        };
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(unreadable)?
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(unreadable)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Ok(Extracted::NotApplicable);
        }
        // A photo that does not decode is "read, no text", not a failure.
        Ok(decode_page(bytes).map_or(Extracted::NotApplicable, |page| {
            Extracted::PageImages(vec![page])
        }))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use image::{GrayImage, Luma};
    use tempfile::TempDir;

    use super::*;
    use crate::test_archive::{test_archive, TestContent};
    use crate::test_support::write;

    fn png_with_dark_block(width: u32, height: u32) -> Vec<u8> {
        let mut picture = GrayImage::from_pixel(width, height, Luma([255]));
        for y in 2..6 {
            for x in 10..20 {
                picture.put_pixel(x, y, Luma([0]));
            }
        }
        let mut bytes = Vec::new();
        picture
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    /// Ink along all four edges, so trimming leaves the size alone.
    fn png_with_ink_on_every_edge(width: u32, height: u32) -> Vec<u8> {
        let mut picture = GrayImage::from_pixel(width, height, Luma([255]));
        for (x, y, pixel) in picture.enumerate_pixels_mut() {
            if x < 8 || y < 8 || x >= width - 8 || y >= height - 8 {
                *pixel = Luma([0]);
            }
        }
        let mut bytes = Vec::new();
        picture
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn pages(path: &Path) -> Vec<PageImage> {
        match ImageFileText.extract(path).unwrap() {
            Extracted::PageImages(pages) => pages,
            other => panic!("expected page images, got {other:?}"),
        }
    }

    #[test]
    fn a_png_becomes_one_grey_page() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "scan.PNG", &png_with_dark_block(40, 20));
        let found = pages(&path);
        assert_eq!(found.len(), 1);
        let page = &found[0];
        // The block and its margin fill the whole 40 x 20 picture, which is
        // then scaled up four times.
        assert_eq!((page.width, page.height), (160, 80));
        let at = |x: usize, y: usize| page.pixels[y * 160 + x];
        assert_eq!(at(12 * 4 + 2, 3 * 4 + 2), 0);
        assert_eq!(at(30 * 4, 15 * 4), 255);
    }

    #[test]
    fn a_page_wider_than_the_cap_is_scaled_down() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "wide.png", &png_with_ink_on_every_edge(5000, 100));
        let page = &pages(&path)[0];
        assert_eq!((page.width, page.height), (4000, 80));
        assert_eq!(page.pixels.len(), 4000 * 80);
    }

    #[test]
    fn a_broken_photo_and_other_files_are_not_applicable() {
        let dir = TempDir::new().unwrap();
        let archive = test_archive();
        let photo = archive
            .archive
            .iter()
            .find_map(|file| match &file.content {
                TestContent::Photo(bytes) => Some(bytes.clone()),
                TestContent::Text(_) => None,
            })
            .unwrap();
        for name in ["broken.jpg", "photo.heic", "doc.pdf"] {
            let path = write(&dir, name, &photo);
            assert_eq!(
                ImageFileText.extract(&path).unwrap(),
                Extracted::NotApplicable,
                "{name}"
            );
        }
        let valid_but_heic = write(&dir, "valid.heic", &png_with_dark_block(40, 20));
        assert_eq!(
            ImageFileText.extract(&valid_but_heic).unwrap(),
            Extracted::NotApplicable
        );
    }
}
