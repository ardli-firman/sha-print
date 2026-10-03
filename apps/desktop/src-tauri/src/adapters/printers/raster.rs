//! PWG Raster decoding before submission through the server's installed printer driver.
use crate::domain::AppError;

pub struct RasterPage {
    pub width: u32,
    pub height: u32,
    pub dpi: [u32; 2],
    /// Top-down BGRX pixels, compatible with a 32-bit Windows DIB.
    pub pixels: Vec<u8>,
}

const HEADER_BYTES: usize = 1796;
const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;
const MAX_PAGES: usize = 1000;

/// Collects small raster documents into a bounded snapshot for inspection and tests.
/// Native printing uses `pwg_pages` to keep one decoded page in memory.
pub fn decode_pwg(document: &[u8]) -> Result<Vec<RasterPage>, AppError> {
    let mut pages = Vec::new();
    let mut total = 0usize;
    for page in pwg_pages(document)? {
        let page = page?;
        total = total.checked_add(page.pixels.len()).ok_or_else(invalid)?;
        if total > MAX_DECODED_BYTES {
            return Err(invalid());
        }
        pages.push(page);
    }
    Ok(pages)
}

/// Streams one bounded page at a time. The Windows adapter first consumes this iterator to
/// validate the complete job, then replays it to the installed driver without retaining all pages.
/// Supports the advertised sgray_8 and srgb_8 layouts (PWG 5102.4, sections 4.3 and 4.4).
pub fn pwg_pages(document: &[u8]) -> Result<PwgPages<'_>, AppError> {
    if !document.starts_with(b"RaS2") || document.len() == 4 {
        return Err(invalid());
    }
    Ok(PwgPages {
        input: &document[4..],
        count: 0,
    })
}

pub struct PwgPages<'a> {
    input: &'a [u8],
    count: usize,
}

impl Iterator for PwgPages<'_> {
    type Item = Result<RasterPage, AppError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.input.is_empty() {
            return None;
        }
        let page = if self.count == MAX_PAGES {
            Err(invalid())
        } else {
            decode_page(&mut self.input)
        };
        self.count += 1;
        if page.is_err() {
            self.input = &[];
        }
        Some(page)
    }
}

fn decode_page(input: &mut &[u8]) -> Result<RasterPage, AppError> {
    let header = take(input, HEADER_BYTES)?;
    let field = |offset: usize| -> u32 {
        u32::from_be_bytes([
            header[offset],
            header[offset + 1],
            header[offset + 2],
            header[offset + 3],
        ])
    };
    let width = field(372);
    let height = field(376);
    let dpi = [field(276), field(280)];
    let colors = match (field(400), field(420)) {
        (18, 1) => 1usize,
        (19, 3) => 3usize,
        _ => return Err(invalid()),
    };
    if width == 0
        || height == 0
        || width > 20_000
        || height > 20_000
        || dpi.iter().any(|dpi| *dpi == 0 || *dpi > 2400)
        || field(384) != 8
        || field(388) != colors as u32 * 8
        || field(396) != 0
        || field(392) != width * colors as u32
    {
        return Err(invalid());
    }
    let row_bytes = width as usize * 4;
    let size = row_bytes.checked_mul(height as usize).ok_or_else(invalid)?;
    if size > MAX_DECODED_BYTES {
        return Err(invalid());
    }
    let mut pixels = vec![0u8; size];
    let mut y = 0usize;
    while y < height as usize {
        let repeats = usize::from(take(input, 1)?[0]) + 1;
        if repeats > height as usize - y {
            return Err(invalid());
        }
        let start = y * row_bytes;
        let row = &mut pixels[start..start + row_bytes];
        let mut x = 0usize;
        while x < width as usize {
            let control = take(input, 1)?[0];
            let count = if control < 128 {
                usize::from(control) + 1
            } else {
                if control == 128 {
                    return Err(invalid());
                }
                257 - usize::from(control)
            };
            if count > width as usize - x {
                return Err(invalid());
            }
            let encoded = take(
                input,
                if control < 128 {
                    colors
                } else {
                    count * colors
                },
            )?;
            for pixel in 0..count {
                let source = if control < 128 { 0 } else { pixel * colors };
                let rgb = &encoded[source..source + colors];
                let bgr = if colors == 3 {
                    [rgb[2], rgb[1], rgb[0], 0]
                } else {
                    [rgb[0], rgb[0], rgb[0], 0]
                };
                row[(x + pixel) * 4..(x + pixel + 1) * 4].copy_from_slice(&bgr);
            }
            x += count;
        }
        for repeat in 1..repeats {
            pixels.copy_within(start..start + row_bytes, start + repeat * row_bytes);
        }
        y += repeats;
    }
    Ok(RasterPage {
        width,
        height,
        dpi,
        pixels,
    })
}

fn take<'a>(input: &mut &'a [u8], count: usize) -> Result<&'a [u8], AppError> {
    if count > input.len() {
        return Err(invalid());
    }
    let (bytes, remaining) = input.split_at(count);
    *input = remaining;
    Ok(bytes)
}

fn invalid() -> AppError {
    AppError::invalid_input("the PWG Raster document is invalid or exceeds supported limits")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: u32, height: u32, colors: u32) -> Vec<u8> {
        let mut header = vec![0; 1796];
        for (offset, value) in [
            (276, 300),
            (280, 300),
            (372, width),
            (376, height),
            (384, 8),
            (388, colors * 8),
            (392, width * colors),
            (400, if colors == 3 { 19 } else { 18 }),
            (420, colors),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        header
    }

    #[test]
    fn pwg_standard_color_sample_matches_every_pixel() {
        // PWG 5102.4 section 4.4.2, the independent 8x8 sample and its published encoding.
        let encoded = hex::decode(concat!(
            "0000ffffff02ffff0003ffffff",
            "00feffff000000ffffff0002ffffffff00ff00ffffff",
            "0001ffff0002ffffff0200ff00",
            "0002ffff0002ffffffff00ff00ffffff",
            "0000ffffff02ffff0003ffffff",
            "0007ffffff",
            "0107ff0000",
        ))
        .expect("standard sample is hex");
        assert_eq!(encoded.len(), 87);
        let mut document = b"RaS2".to_vec();
        document.extend(header(8, 8, 3));
        document.extend(encoded);
        let pages = decode_pwg(&document).expect("decodes the standard sample");
        let expected: Vec<u8> = concat!(
            "WYYYWWWW", "YBYWWWGW", "YYWWWGGG", "YYYWWWGW", "WYYYWWWW", "WWWWWWWW", "RRRRRRRR",
            "RRRRRRRR",
        )
        .bytes()
        .flat_map(|pixel| match pixel {
            b'W' => [255, 255, 255, 0],
            b'Y' => [0, 255, 255, 0],
            b'B' => [255, 0, 0, 0],
            b'G' => [0, 255, 0, 0],
            b'R' => [0, 0, 255, 0],
            _ => unreachable!(),
        })
        .collect();
        assert_eq!(pages[0].pixels, expected);
    }

    #[test]
    fn a_long_job_is_decoded_one_page_at_a_time() {
        let mut document = b"RaS2".to_vec();
        for _ in 0..10 {
            document.extend(header(2480, 3508, 1));
            let mut rows = 3508usize;
            while rows > 0 {
                let repeats = rows.min(256);
                document.push((repeats - 1) as u8);
                let mut pixels = 2480usize;
                while pixels > 0 {
                    let count = pixels.min(128);
                    document.extend([(count - 1) as u8, 255]);
                    pixels -= count;
                }
                rows -= repeats;
            }
        }
        let mut count = 0;
        let mut decoded_bytes = 0;
        for page in pwg_pages(&document).expect("opens the long job") {
            let page = page.expect("decodes the next page");
            assert_eq!(page.pixels.len(), 2480 * 3508 * 4);
            assert_eq!(&page.pixels[..4], &[255, 255, 255, 0]);
            decoded_bytes += page.pixels.len();
            count += 1;
        }
        assert_eq!(count, 10);
        assert!(decoded_bytes > MAX_DECODED_BYTES);
    }

    #[test]
    fn repeated_rows_and_literal_rgb_pixels_become_page_pixels_not_raw_bytes() {
        let mut document = b"RaS2".to_vec();
        document.extend(header(2, 2, 3));
        // Two identical rows; two literal RGB pixels, red then blue.
        document.extend([1, 255, 255, 0, 0, 0, 0, 255]);
        let pages = decode_pwg(&document).expect("decodes a PWG page");
        assert_eq!(pages.len(), 1);
        assert_eq!(
            (pages[0].width, pages[0].height, pages[0].dpi),
            (2, 2, [300, 300])
        );
        assert_eq!(pages[0].pixels, [0, 0, 255, 0, 255, 0, 0, 0].repeat(2));
    }

    #[test]
    fn grayscale_runs_and_multiple_pages_preserve_page_boundaries() {
        let mut document = b"RaS2".to_vec();
        document.extend(header(2, 1, 1));
        document.extend([0, 1, 64]);
        document.extend(header(1, 1, 1));
        document.extend([0, 0, 255]);
        let pages = decode_pwg(&document).expect("decodes both pages");
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].pixels, [64, 64, 64, 0].repeat(2));
        assert_eq!(pages[1].pixels, [255, 255, 255, 0]);
    }

    #[test]
    fn truncated_or_overflowing_runs_and_unsupported_headers_are_rejected() {
        let mut valid = b"RaS2".to_vec();
        valid.extend(header(2, 1, 1));
        valid.extend([0, 1, 64]);
        for length in 0..valid.len() {
            assert!(
                decode_pwg(&valid[..length]).is_err(),
                "accepted truncation at {length}"
            );
        }
        for (offset, value) in [
            (372, u32::MAX),
            (396, 1),
            (400, 6),
            (384, 16),
            (392, 3),
            (276, 0),
        ] {
            let mut bad = valid.clone();
            bad[4 + offset..8 + offset].copy_from_slice(&value.to_be_bytes());
            assert!(decode_pwg(&bad).is_err());
        }
        let mut bad_rows = valid.clone();
        bad_rows[1800] = 1;
        assert!(decode_pwg(&bad_rows).is_err());
        let mut bad_pixels = valid;
        bad_pixels[1801] = 2;
        assert!(decode_pwg(&bad_pixels).is_err());
    }
}
