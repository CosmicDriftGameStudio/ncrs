//! Cutting the empty paper off a scanned page before text recognition.

use image::imageops::{self, FilterType};
use image::GrayImage;

use crate::text::PageImage;

/// Paper and scan noise are lighter than this; ink is darker.
const DARK_BELOW: u8 = 160;
/// A row or column with fewer dark pixels is dust, not content.
const MIN_DARK_PER_LINE: usize = 3;
const MARGIN_PIXELS: usize = 24;
/// Recognition reads small print best when the content's longer side has about
/// this many pixels, which stays below `MAX_SCAN_SIDE_PIXELS`.
const TARGET_SIDE_PIXELS: u64 = 2400;
const MAX_UPSCALE: u64 = 4;
/// A scale this close to 1 is not worth a resampling.
const NO_SCALE_BELOW_PERCENT: u64 = 105;

fn pixel_count(side: u32) -> usize {
    usize::try_from(side).unwrap_or(usize::MAX)
}

/// The page cut to its content with a margin, and scaled so the content's
/// longer side has about `TARGET_SIDE_PIXELS`. A page without content stays as
/// it is.
pub fn trim_to_content(page: PageImage) -> PageImage {
    let (width, height) = (pixel_count(page.width), pixel_count(page.height));
    if width == 0 || height == 0 || page.pixels.len() != width.saturating_mul(height) {
        return page;
    }
    let mut dark_in_row = vec![0; height];
    let mut dark_in_column = vec![0; width];
    for (row_count, line) in dark_in_row.iter_mut().zip(page.pixels.chunks_exact(width)) {
        for (column_count, &pixel) in dark_in_column.iter_mut().zip(line) {
            if pixel < DARK_BELOW {
                *row_count += 1;
                *column_count += 1;
            }
        }
    }
    let is_content = |count: &usize| *count >= MIN_DARK_PER_LINE;
    let (Some(top), Some(bottom), Some(left), Some(right)) = (
        dark_in_row.iter().position(is_content),
        dark_in_row.iter().rposition(is_content),
        dark_in_column.iter().position(is_content),
        dark_in_column.iter().rposition(is_content),
    ) else {
        return page;
    };
    let first_column = left.saturating_sub(MARGIN_PIXELS);
    let last_column = right.saturating_add(MARGIN_PIXELS).min(width - 1);
    let first_row = top.saturating_sub(MARGIN_PIXELS);
    let last_row = bottom.saturating_add(MARGIN_PIXELS).min(height - 1);
    let box_width = last_column - first_column + 1;
    let box_height = last_row - first_row + 1;
    let cropped: Vec<u8> = page
        .pixels
        .chunks_exact(width)
        .skip(first_row)
        .take(box_height)
        .flat_map(|line| line.iter().skip(first_column).take(box_width).copied())
        .collect();

    let longer_side = u64::try_from(box_width.max(box_height)).unwrap_or(u64::MAX);
    let scaled_up = TARGET_SIDE_PIXELS * 100 >= longer_side * NO_SCALE_BELOW_PERCENT;
    let (numerator, denominator) = if TARGET_SIDE_PIXELS > MAX_UPSCALE * longer_side {
        (MAX_UPSCALE, 1)
    } else {
        (TARGET_SIDE_PIXELS, longer_side)
    };
    let scaled = |length: usize| {
        let length_wide = u64::try_from(length).unwrap_or(u64::MAX);
        let pixels = (length_wide * numerator).div_ceil(denominator);
        u32::try_from(pixels.max(1)).unwrap_or(u32::MAX)
    };
    let (Ok(cropped_width), Ok(cropped_height)) =
        (u32::try_from(box_width), u32::try_from(box_height))
    else {
        return page;
    };
    let Some(cropped_grey) = GrayImage::from_raw(cropped_width, cropped_height, cropped) else {
        return page;
    };
    let finished = if scaled_up {
        imageops::resize(
            &cropped_grey,
            scaled(box_width),
            scaled(box_height),
            FilterType::CatmullRom,
        )
    } else {
        cropped_grey
    };
    PageImage {
        width: finished.width(),
        height: finished.height(),
        pixels: finished.into_raw(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const INK: u8 = 0;
    const PAPER: u8 = 255;

    fn page(width: u32, height: u32) -> PageImage {
        PageImage {
            width,
            height,
            pixels: vec![PAPER; pixel_count(width) * pixel_count(height)],
        }
    }

    fn ink(page: &mut PageImage, left: u32, top: u32, width: u32, height: u32) {
        for y in top..top + height {
            for x in left..left + width {
                let at = pixel_count(y) * pixel_count(page.width) + pixel_count(x);
                page.pixels[at] = INK;
            }
        }
    }

    fn row_is_paper(page: &PageImage, y: u32) -> bool {
        let from = pixel_count(y) * pixel_count(page.width);
        page.pixels[from..from + pixel_count(page.width)]
            .iter()
            .all(|pixel| *pixel == PAPER)
    }

    #[test]
    fn a_small_block_in_the_middle_is_cut_out_and_scaled_to_the_target_side() {
        let mut scan = page(1600, 2200);
        ink(&mut scan, 550, 750, 500, 700);

        let trimmed = trim_to_content(scan);

        // 500 x 700 plus 24 px on every side is 548 x 748: 2400 / 748 times.
        assert!(trimmed.height.abs_diff(2400) <= 2, "{}", trimmed.height);
        let expected_width = 548.0 * f64::from(trimmed.height) / 748.0;
        assert!(
            (f64::from(trimmed.width) - expected_width).abs() <= 2.0,
            "{} vs {expected_width}",
            trimmed.width
        );
        let scale = f64::from(trimmed.height) / 748.0;
        let margin_rows = (24.0 * scale * 0.5) as u32;
        assert!(!row_is_paper(&trimmed, trimmed.height.div_euclid(2)));
        for y in 0..margin_rows {
            assert!(row_is_paper(&trimmed, y), "row {y}");
        }
        let white_rows_at_top = (0..trimmed.height)
            .take_while(|y| row_is_paper(&trimmed, *y))
            .count();
        assert!(
            f64::from(u32::try_from(white_rows_at_top).unwrap()) <= 24.0 * scale + 3.0,
            "{white_rows_at_top} white rows on top"
        );
    }

    #[test]
    fn a_blank_page_stays_as_it_is() {
        let scan = page(800, 1000);
        assert_eq!(trim_to_content(scan.clone()), scan);
    }

    #[test]
    fn a_page_that_is_full_keeps_its_size() {
        let mut scan = page(2000, 3000);
        ink(&mut scan, 0, 0, 2000, 3);
        ink(&mut scan, 0, 2997, 2000, 3);
        ink(&mut scan, 0, 0, 3, 3000);
        ink(&mut scan, 1997, 0, 3, 3000);

        let trimmed = trim_to_content(scan);

        assert_eq!((trimmed.width, trimmed.height), (2000, 3000));
    }

    #[test]
    fn a_speck_of_dust_in_a_corner_is_not_content() {
        let mut scan = page(1000, 1000);
        ink(&mut scan, 450, 450, 100, 100);
        ink(&mut scan, 0, 0, 1, 1);
        ink(&mut scan, 998, 998, 2, 1);

        let trimmed = trim_to_content(scan);

        // The block is 100 px plus margins: 148 px, scaled up four times.
        assert_eq!((trimmed.width, trimmed.height), (592, 592));
    }
}
