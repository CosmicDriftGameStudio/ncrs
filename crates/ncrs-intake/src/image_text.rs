//! Photos and scans stored as image files: they have no text of their own, so
//! the file is handed on as one page image for the image sources (OCR).

use std::fs::File;
use std::io::{Cursor, Read as _};
use std::path::Path;

use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageDecoder as _, ImageReader, Limits};

use crate::page_trim::trim_to_content;
use crate::pdf_text::{MAX_SCAN_PAGES, MAX_SCAN_SIDE_PIXELS};
use crate::text::{Extracted, PageImage, TextError, TextSource};

/// The image formats this source decodes; heic is not among them.
pub const EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "tif", "tiff"];
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

fn decode_page(bytes: &[u8]) -> Option<PageImage> {
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
    Some(grey_page(picture))
}

fn is_tiff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*")
}

/// The first page as for every image; a TIFF adds its further pages, up to
/// the cap of a scanned PDF. A later page in a form that cannot be read is
/// left out, the others stay.
fn decode_pages(bytes: &[u8]) -> Vec<PageImage> {
    let Some(first) = decode_page(bytes) else {
        return Vec::new();
    };
    let mut pages = vec![first];
    if !is_tiff(bytes) {
        return pages;
    }
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = usize::try_from(MAX_DECODE_BYTES).unwrap_or(usize::MAX);
    let Ok(plain_decoder) = tiff::decoder::Decoder::new(Cursor::new(bytes)) else {
        return pages;
    };
    let mut decoder = plain_decoder.with_limits(limits);
    for page in 1..MAX_SCAN_PAGES {
        if !decoder.more_images() || decoder.seek_to_image(page).is_err() {
            break;
        }
        if let Some(picture) = later_tiff_page(&mut decoder) {
            pages.push(grey_page(picture));
        }
    }
    pages
}

fn later_tiff_page(decoder: &mut tiff::decoder::Decoder<Cursor<&[u8]>>) -> Option<DynamicImage> {
    use tiff::decoder::DecodingResult;
    use tiff::ColorType;

    let (width, height) = decoder.dimensions().ok()?;
    if width > MAX_SIDE_PIXELS || height > MAX_SIDE_PIXELS {
        return None;
    }
    let colour = decoder.colortype().ok()?;
    match (colour, decoder.read_image().ok()?) {
        (ColorType::Gray(1), DecodingResult::U8(packed)) => {
            let row_bytes = usize::try_from(width.div_ceil(8)).ok()?;
            let columns = usize::try_from(width).ok()?;
            let pixels = packed
                .chunks_exact(row_bytes)
                .flat_map(|row| {
                    (0..columns).map(move |x| {
                        let bit = row
                            .get(x.div_euclid(8))
                            .map_or(0, |byte| (byte >> (7 - x % 8)) & 1);
                        bit * u8::MAX
                    })
                })
                .collect();
            image::GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
        }
        (ColorType::Gray(8), DecodingResult::U8(pixels)) => {
            image::GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
        }
        (ColorType::GrayA(8), DecodingResult::U8(pixels)) => {
            image::GrayAlphaImage::from_raw(width, height, pixels).map(DynamicImage::ImageLumaA8)
        }
        (ColorType::RGB(8), DecodingResult::U8(pixels)) => {
            image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
        }
        (ColorType::RGBA(8), DecodingResult::U8(pixels)) => {
            image::RgbaImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgba8)
        }
        (ColorType::Gray(16), DecodingResult::U16(pixels)) => {
            image::ImageBuffer::from_raw(width, height, pixels).map(DynamicImage::ImageLuma16)
        }
        (ColorType::RGB(16), DecodingResult::U16(pixels)) => {
            image::ImageBuffer::from_raw(width, height, pixels).map(DynamicImage::ImageRgb16)
        }
        _ => None,
    }
}

fn grey_page(picture: DynamicImage) -> PageImage {
    let mut grey = picture.into_luma8();
    let longer_side = f64::from(grey.width().max(grey.height()));
    let cap = f64::from(MAX_SCAN_SIDE_PIXELS);
    if longer_side > cap {
        let scaled = |side: u32| {
            // Rounded and at most the cap, so the value fits a u32.
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
    trim_to_content(PageImage {
        width: grey.width(),
        height: grey.height(),
        pixels: grey.into_raw(),
    })
}

impl TextSource for ImageFileText {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        if !is_image_extension(path) {
            return Ok(Extracted::NotApplicable);
        }
        let file = File::open(path).map_err(|err| TextError::Io {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
        self.extract_open(path, &file)
    }

    fn extract_open(&self, path: &Path, file: &File) -> Result<Extracted, TextError> {
        if !is_image_extension(path) {
            return Ok(Extracted::NotApplicable);
        }
        let unreadable = |err: std::io::Error| TextError::Io {
            path: path.to_path_buf(),
            reason: err.to_string(),
        };
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(unreadable)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Ok(Extracted::NotApplicable);
        }
        // A photo that does not decode is "read, no text", not a failure.
        let pages = decode_pages(&bytes);
        Ok(if pages.is_empty() {
            Extracted::NotApplicable
        } else {
            Extracted::PageImages(pages)
        })
    }
}

#[cfg(test)]
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

    fn grey_with_ink_on_every_edge(width: u32, height: u32, ink: u8) -> Vec<u8> {
        let mut picture = GrayImage::from_pixel(width, height, Luma([255]));
        for (x, y, pixel) in picture.enumerate_pixels_mut() {
            if x < 8 || y < 8 || x >= width - 8 || y >= height - 8 {
                *pixel = Luma([ink]);
            }
        }
        picture.into_raw()
    }

    fn tiff_of(pages: &[(u32, u32, u8)]) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        let mut encoder = tiff::encoder::TiffEncoder::new(&mut bytes).unwrap();
        for &(width, height, ink) in pages {
            encoder
                .write_image::<tiff::encoder::colortype::Gray8>(
                    width,
                    height,
                    &grey_with_ink_on_every_edge(width, height, ink),
                )
                .unwrap();
        }
        bytes.into_inner()
    }

    #[test]
    fn every_page_of_a_tiff_becomes_a_page_image_in_order() {
        let dir = TempDir::new().unwrap();
        let path = write(
            &dir,
            "scan.tif",
            &tiff_of(&[(1000, 1400, 0), (1200, 1000, 40), (1000, 1000, 80)]),
        );
        let found = pages(&path);
        // Trimming scales small pages up; the sides keep their ratio.
        let shapes: Vec<std::cmp::Ordering> = found
            .iter()
            .map(|page| page.width.cmp(&page.height))
            .collect();
        assert_eq!(
            shapes,
            vec![
                std::cmp::Ordering::Less,
                std::cmp::Ordering::Greater,
                std::cmp::Ordering::Equal
            ]
        );
        let corner: Vec<u8> = found.iter().map(|page| page.pixels[0]).collect();
        assert_eq!(corner, vec![0, 40, 80]);
    }

    #[test]
    fn a_tiff_gives_at_most_as_many_pages_as_a_scanned_pdf() {
        let many = vec![(64, 64, 0); MAX_SCAN_PAGES + 3];
        assert_eq!(decode_pages(&tiff_of(&many)).len(), MAX_SCAN_PAGES);
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
