//! Draw a backend-owned diagnostic page into a PWG Raster stream and submit it through
//! the installed native CUPS Client queue.
//! No direct IPP proxy submission, saved-preference mutation, document export, or raw error text.

use std::ffi::CString;

use crate::adapters::printers::cups::{
    build_cups_job_options, query_cups_active_jobs, query_cups_destinations,
    query_cups_printer_ppd, validate_pwg_raster_stream, CupsHttpConnection, CupsOption,
    CupsServerEndpoint, LibCups, HTTP_STATUS_CONTINUE, IPP_PSTATE_STOPPED, IPP_STATUS_OK,
    IPP_STATUS_OK_CONFLICT, IPP_STATUS_OK_SUBST, STREAM_CHUNK_BYTES,
};
use crate::adapters::printers::media::media_spec;
use crate::adapters::queue_installation::cups::{
    extract_valid_default_pagesize_from_ppd, resolve_default_pagesize_from_lpoptions,
    resolve_default_pagesize_from_ppd,
};
use crate::application::{
    DuplexMode, NativeTestPrint, PrintOrientation, PrintSettings, SubmissionOutcome, TestMedia,
};
use crate::domain::{AppError, RecognisedClientQueue, SpoolerRecord, SpoolerRecordClassification};

/// Classic 8x8 font bitmap covering ASCII 32 (' ') through 126 ('~').
/// Each character is defined by 8 bytes representing rows 0-7 from top to bottom.
#[rustfmt::skip]
const FONT_8X8: [[u8; 8]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // 32 ' '
    [0x18, 0x3c, 0x3c, 0x18, 0x18, 0x00, 0x18, 0x00], // 33 '!'
    [0x66, 0x66, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00], // 34 '"'
    [0x6c, 0x6c, 0xfe, 0x6c, 0xfe, 0x6c, 0x6c, 0x00], // 35 '#'
    [0x18, 0x3e, 0x60, 0x3c, 0x06, 0x7c, 0x18, 0x00], // 36 '$'
    [0x00, 0x63, 0x66, 0x0c, 0x18, 0x33, 0x63, 0x00], // 37 '%'
    [0x38, 0x6c, 0x38, 0x76, 0xdc, 0xcc, 0x76, 0x00], // 38 '&'
    [0x30, 0x30, 0x10, 0x20, 0x00, 0x00, 0x00, 0x00], // 39 '\''
    [0x0c, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0c, 0x00], // 40 '('
    [0x30, 0x18, 0x0c, 0x0c, 0x0c, 0x18, 0x30, 0x00], // 41 ')'
    [0x00, 0x66, 0x3c, 0xff, 0x3c, 0x66, 0x00, 0x00], // 42 '*'
    [0x00, 0x18, 0x18, 0x7e, 0x18, 0x18, 0x00, 0x00], // 43 '+'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30], // 44 ','
    [0x00, 0x00, 0x00, 0x7e, 0x00, 0x00, 0x00, 0x00], // 45 '-'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00], // 46 '.'
    [0x06, 0x0c, 0x18, 0x30, 0x60, 0xc0, 0x80, 0x00], // 47 '/'
    [0x3c, 0x66, 0x6e, 0x76, 0x66, 0x66, 0x3c, 0x00], // 48 '0'
    [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7e, 0x00], // 49 '1'
    [0x3c, 0x66, 0x06, 0x0c, 0x18, 0x30, 0x7e, 0x00], // 50 '2'
    [0x3c, 0x66, 0x06, 0x1c, 0x06, 0x66, 0x3c, 0x00], // 51 '3'
    [0x0c, 0x1c, 0x3c, 0x6c, 0xfe, 0x0c, 0x0c, 0x00], // 52 '4'
    [0x7e, 0x60, 0x7c, 0x06, 0x06, 0x66, 0x3c, 0x00], // 53 '5'
    [0x3c, 0x66, 0x60, 0x7c, 0x66, 0x66, 0x3c, 0x00], // 54 '6'
    [0x7e, 0x66, 0x0c, 0x18, 0x18, 0x18, 0x18, 0x00], // 55 '7'
    [0x3c, 0x66, 0x66, 0x3c, 0x66, 0x66, 0x3c, 0x00], // 56 '8'
    [0x3c, 0x66, 0x66, 0x3e, 0x06, 0x66, 0x3c, 0x00], // 57 '9'
    [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00], // 58 ':'
    [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x30, 0x00], // 59 ';'
    [0x06, 0x0c, 0x18, 0x30, 0x18, 0x0c, 0x06, 0x00], // 60 '<'
    [0x00, 0x00, 0x7e, 0x00, 0x7e, 0x00, 0x00, 0x00], // 61 '='
    [0x60, 0x30, 0x18, 0x0c, 0x18, 0x30, 0x60, 0x00], // 62 '>'
    [0x3c, 0x66, 0x0c, 0x18, 0x18, 0x00, 0x18, 0x00], // 63 '?'
    [0x3c, 0x66, 0x6e, 0x6a, 0x6e, 0x60, 0x3c, 0x00], // 64 '@'
    [0x18, 0x3c, 0x66, 0x66, 0x7e, 0x66, 0x66, 0x00], // 65 'A'
    [0x7c, 0x66, 0x66, 0x7c, 0x66, 0x66, 0x7c, 0x00], // 66 'B'
    [0x3c, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3c, 0x00], // 67 'C'
    [0x78, 0x6c, 0x66, 0x66, 0x66, 0x6c, 0x78, 0x00], // 68 'D'
    [0x7e, 0x60, 0x60, 0x78, 0x60, 0x60, 0x7e, 0x00], // 69 'E'
    [0x7e, 0x60, 0x60, 0x78, 0x60, 0x60, 0x60, 0x00], // 70 'F'
    [0x3c, 0x66, 0x60, 0x6e, 0x66, 0x66, 0x3c, 0x00], // 71 'G'
    [0x66, 0x66, 0x66, 0x7e, 0x66, 0x66, 0x66, 0x00], // 72 'H'
    [0x3c, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00], // 73 'I'
    [0x0e, 0x06, 0x06, 0x06, 0x06, 0x66, 0x3c, 0x00], // 74 'J'
    [0x66, 0x6c, 0x78, 0x70, 0x78, 0x6c, 0x66, 0x00], // 75 'K'
    [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7e, 0x00], // 76 'L'
    [0x63, 0x77, 0x7f, 0x6b, 0x63, 0x63, 0x63, 0x00], // 77 'M'
    [0x66, 0x76, 0x7e, 0x7e, 0x6e, 0x66, 0x66, 0x00], // 78 'N'
    [0x3c, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00], // 79 'O'
    [0x7c, 0x66, 0x66, 0x7c, 0x60, 0x60, 0x60, 0x00], // 80 'P'
    [0x3c, 0x66, 0x66, 0x66, 0x6a, 0x6c, 0x36, 0x00], // 81 'Q'
    [0x7c, 0x66, 0x66, 0x7c, 0x6c, 0x66, 0x66, 0x00], // 82 'R'
    [0x3c, 0x66, 0x60, 0x3c, 0x06, 0x66, 0x3c, 0x00], // 83 'S'
    [0x7e, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // 84 'T'
    [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x00], // 85 'U'
    [0x66, 0x66, 0x66, 0x66, 0x66, 0x3c, 0x18, 0x00], // 86 'V'
    [0x63, 0x63, 0x63, 0x6b, 0x7f, 0x77, 0x63, 0x00], // 87 'W'
    [0x66, 0x66, 0x3c, 0x18, 0x3c, 0x66, 0x66, 0x00], // 88 'X'
    [0x66, 0x66, 0x66, 0x3c, 0x18, 0x18, 0x18, 0x00], // 89 'Y'
    [0x7e, 0x06, 0x0c, 0x18, 0x30, 0x60, 0x7e, 0x00], // 90 'Z'
    [0x3c, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3c, 0x00], // 91 '['
    [0xc0, 0x60, 0x30, 0x18, 0x0c, 0x06, 0x02, 0x00], // 92 '\\'
    [0x3c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x3c, 0x00], // 93 ']'
    [0x10, 0x38, 0x6c, 0xc6, 0x00, 0x00, 0x00, 0x00], // 94 '^'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff], // 95 '_'
    [0x30, 0x18, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00], // 96 '`'
    [0x00, 0x00, 0x3c, 0x06, 0x3e, 0x66, 0x3e, 0x00], // 97 'a'
    [0x60, 0x60, 0x7c, 0x66, 0x66, 0x66, 0x7c, 0x00], // 98 'b'
    [0x00, 0x00, 0x3c, 0x66, 0x60, 0x66, 0x3c, 0x00], // 99 'c'
    [0x06, 0x06, 0x3e, 0x66, 0x66, 0x66, 0x3e, 0x00], // 100 'd'
    [0x00, 0x00, 0x3c, 0x66, 0x7e, 0x60, 0x3c, 0x00], // 101 'e'
    [0x0e, 0x18, 0x7c, 0x18, 0x18, 0x18, 0x18, 0x00], // 102 'f'
    [0x00, 0x00, 0x3e, 0x66, 0x66, 0x3e, 0x06, 0x7c], // 103 'g'
    [0x60, 0x60, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x00], // 104 'h'
    [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3c, 0x00], // 105 'i'
    [0x06, 0x00, 0x06, 0x06, 0x06, 0x06, 0x66, 0x3c], // 106 'j'
    [0x60, 0x60, 0x66, 0x6c, 0x78, 0x6c, 0x66, 0x00], // 107 'k'
    [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3c, 0x00], // 108 'l'
    [0x00, 0x00, 0x66, 0x7f, 0x7f, 0x6b, 0x63, 0x00], // 109 'm'
    [0x00, 0x00, 0x7c, 0x66, 0x66, 0x66, 0x66, 0x00], // 110 'n'
    [0x00, 0x00, 0x3c, 0x66, 0x66, 0x66, 0x3c, 0x00], // 111 'o'
    [0x00, 0x00, 0x7c, 0x66, 0x66, 0x7c, 0x60, 0x60], // 112 'p'
    [0x00, 0x00, 0x3e, 0x66, 0x66, 0x3e, 0x06, 0x06], // 113 'q'
    [0x00, 0x00, 0x7c, 0x66, 0x60, 0x60, 0x60, 0x00], // 114 'r'
    [0x00, 0x00, 0x3e, 0x60, 0x3c, 0x06, 0x7c, 0x00], // 115 's'
    [0x18, 0x18, 0x7e, 0x18, 0x18, 0x18, 0x0e, 0x00], // 116 't'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3e, 0x00], // 117 'u'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x3c, 0x18, 0x00], // 118 'v'
    [0x00, 0x00, 0x63, 0x6b, 0x7f, 0x77, 0x63, 0x00], // 119 'w'
    [0x00, 0x00, 0x66, 0x3c, 0x18, 0x3c, 0x66, 0x00], // 120 'x'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x3e, 0x06, 0x7c], // 121 'y'
    [0x00, 0x00, 0x7e, 0x0c, 0x18, 0x30, 0x7e, 0x00], // 122 'z'
    [0x0e, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0e, 0x00], // 123 '{'
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // 124 '|'
    [0x70, 0x18, 0x18, 0x0e, 0x18, 0x18, 0x70, 0x00], // 125 '}'
    [0x76, 0xdc, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // 126 '~'
];

fn get_glyph(ch: char) -> [u8; 8] {
    let code = ch as usize;
    if (32..=126).contains(&code) {
        FONT_8X8[code - 32]
    } else {
        FONT_8X8[63 - 32] // '?'
    }
}

fn set_pixel(pixels: &mut [u8], width: usize, height: usize, x: usize, y: usize, val: u8) {
    if x < width && y < height {
        pixels[y * width + x] = val;
    }
}

struct InsetRect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

fn draw_rect(pixels: &mut [u8], width: usize, height: usize, rect: InsetRect, thickness: usize) {
    for t in 0..thickness {
        for x in rect.x0..=rect.x1 {
            set_pixel(pixels, width, height, x, rect.y0 + t, 0);
            if rect.y1 >= t {
                set_pixel(pixels, width, height, x, rect.y1 - t, 0);
            }
        }
        for y in rect.y0..=rect.y1 {
            set_pixel(pixels, width, height, rect.x0 + t, y, 0);
            if rect.x1 >= t {
                set_pixel(pixels, width, height, rect.x1 - t, y, 0);
            }
        }
    }
}

fn draw_glyph(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    origin_x: usize,
    origin_y: usize,
    ch: char,
    scale: usize,
) {
    let glyph = get_glyph(ch);
    for (row, &bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if (bits & (1 << (7 - col))) != 0 {
                for sy in 0..scale {
                    for sx in 0..scale {
                        set_pixel(
                            pixels,
                            width,
                            height,
                            origin_x + col * scale + sx,
                            origin_y + row * scale + sy,
                            0,
                        );
                    }
                }
            }
        }
    }
}

fn draw_text(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    text: &str,
    scale: usize,
) {
    let mut cur_x = x;
    for ch in text.chars() {
        draw_glyph(pixels, width, height, cur_x, y, ch, scale);
        cur_x += 8 * scale;
    }
}

/// Generates a valid 1-page, 300 DPI, grayscale PWG Raster diagnostic document.
///
/// Follows the parent specification and Windows parity:
/// - One page, grayscale (18 / sgray_8), one-sided.
/// - Border rectangles at 2 mm, 5 mm, and 10 mm insets.
/// - Center cross of 50 x 50 mm.
/// - Requested media keyword and dimensions identified.
pub fn generate_diagnostic_pwg_raster(media: TestMedia) -> Vec<u8> {
    const DPI: u32 = 300;
    let (w_tenths, h_tenths) = media.dimensions_tenths_mm();
    let width_px = ((u64::from(w_tenths as u32) * u64::from(DPI) + 127) / 254) as usize;
    let height_px = ((u64::from(h_tenths as u32) * u64::from(DPI) + 127) / 254) as usize;

    let mut pixels = vec![255u8; width_px * height_px];

    // Border insets at 2 mm (20 tenths mm), 5 mm (50 tenths mm), and 10 mm (100 tenths mm).
    for inset_tenths in [20u64, 50, 100] {
        let inset_px = ((inset_tenths * u64::from(DPI) + 127) / 254) as usize;
        if width_px > 2 * inset_px && height_px > 2 * inset_px {
            draw_rect(
                &mut pixels,
                width_px,
                height_px,
                InsetRect {
                    x0: inset_px,
                    y0: inset_px,
                    x1: width_px - 1 - inset_px,
                    y1: height_px - 1 - inset_px,
                },
                3,
            );
        }
    }

    // Center cross: 50 x 50 mm (500 x 500 tenths mm = 250 tenths mm on each side of center).
    let cx = width_px / 2;
    let cy = height_px / 2;
    let half_cross_px = ((250u64 * u64::from(DPI) + 127) / 254) as usize;
    let thickness = 3usize;

    let x_start = cx.saturating_sub(half_cross_px);
    let x_end = (cx + half_cross_px).min(width_px.saturating_sub(1));
    for x in x_start..=x_end {
        for t in 0..thickness {
            let y = cy.saturating_sub(thickness / 2) + t;
            set_pixel(&mut pixels, width_px, height_px, x, y, 0);
        }
    }

    let y_start = cy.saturating_sub(half_cross_px);
    let y_end = (cy + half_cross_px).min(height_px.saturating_sub(1));
    for y in y_start..=y_end {
        for t in 0..thickness {
            let x = cx.saturating_sub(thickness / 2) + t;
            set_pixel(&mut pixels, width_px, height_px, x, y, 0);
        }
    }

    // Diagnostic text
    let lines = [
        "ShaPrint diagnostic page".to_owned(),
        "Requested media:".to_owned(),
        media.keyword().to_owned(),
        format!(
            "{} x {} mm",
            f32::from(w_tenths) / 10.0,
            f32::from(h_tenths) / 10.0
        ),
        "1 page, 1 copy, grayscale, one-sided".to_owned(),
        "Borders: 2, 5 and 10 mm from each edge".to_owned(),
        "Center cross: 50 x 50 mm".to_owned(),
        "Compare Requested media with Loaded media.".to_owned(),
    ];

    let start_x = ((150u64 * u64::from(DPI) + 127) / 254) as usize; // ~15 mm from edge
    let start_y = ((180u64 * u64::from(DPI) + 127) / 254) as usize; // ~18 mm from edge
    let line_step_y = ((50u64 * u64::from(DPI) + 127) / 254) as usize; // ~5 mm line step

    for (idx, line) in lines.iter().enumerate() {
        let y = start_y + idx * line_step_y;
        draw_text(&mut pixels, width_px, height_px, start_x, y, line, 3);
    }

    encode_pwg_raster(width_px as u32, height_px as u32, DPI, &pixels)
}

/// Encodes a grayscale 8-bit bitmap into a PWG Raster stream (RaS2, sgray_8).
pub fn encode_pwg_raster(width: u32, height: u32, dpi: u32, pixels: &[u8]) -> Vec<u8> {
    let mut raster = b"RaS2".to_vec();
    let mut header = vec![0u8; 1796];
    let bits_per_color = 8u32;
    let colors = 1u32;
    let color_space = 18u32; // sgray_8
    let bits_per_pixel = 8u32;
    let bytes_per_line = width;

    for (offset, value) in [
        (276usize, dpi),
        (280, dpi),
        (372, width),
        (376, height),
        (384, bits_per_color),
        (388, bits_per_pixel),
        (392, bytes_per_line),
        (396, 0u32),
        (400, color_space),
        (420, colors),
    ] {
        header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    raster.extend(header);

    let w = width as usize;
    let h = height as usize;
    let mut y = 0usize;
    while y < h {
        let row_start = y * w;
        let current_row = &pixels[row_start..row_start + w];
        let mut repeats = 1usize;
        while y + repeats < h && repeats < 256 {
            let next_start = (y + repeats) * w;
            if &pixels[next_start..next_start + w] == current_row {
                repeats += 1;
            } else {
                break;
            }
        }
        raster.push((repeats - 1) as u8);

        let mut x = 0usize;
        while x < w {
            let val = current_row[x];
            let mut run_len = 1usize;
            while x + run_len < w && run_len < 128 && current_row[x + run_len] == val {
                run_len += 1;
            }
            raster.push((run_len - 1) as u8);
            raster.push(val);
            x += run_len;
        }

        y += repeats;
    }

    raster
}

fn lpoptions_cmd(endpoint: &CupsServerEndpoint, cups_name: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("lpoptions");
    let host_arg = endpoint.lpadmin_host_arg();
    cmd.arg("-h").arg(host_arg);
    cmd.arg("-p").arg(cups_name);
    cmd
}

/// Reads the default Requested media for a CUPS Client queue.
pub(crate) fn read_default_media(
    endpoint: &CupsServerEndpoint,
    cups_name: &str,
) -> Result<Option<TestMedia>, AppError> {
    let dests = query_cups_destinations(endpoint)?;
    let Some(dest) = dests.iter().find(|d| d.name == cups_name) else {
        return Err(AppError::queue_unavailable(
            "The native Client queue could not be opened.",
        ));
    };

    let opt_media = dest.options.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case("media") || k.eq_ignore_ascii_case("pagesize") {
            Some(v.clone())
        } else {
            None
        }
    });

    let raw_name = match opt_media {
        Some(name) => Some(name),
        None => query_cups_printer_ppd(endpoint, cups_name)
            .ok()
            .and_then(|ppd| {
                extract_valid_default_pagesize_from_ppd(&ppd)
                    .or_else(|| resolve_default_pagesize_from_ppd(&ppd))
            })
            .or_else(|| {
                let mut cmd = lpoptions_cmd(endpoint, cups_name);
                cmd.arg("-l");
                let out = cmd.output().ok()?;
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    resolve_default_pagesize_from_lpoptions(&stdout)
                } else {
                    None
                }
            })
            .or_else(|| {
                let mut cmd = lpoptions_cmd(endpoint, cups_name);
                let out = cmd.output().ok()?;
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    stdout.split_whitespace().find_map(|part| {
                        if let Some((k, v)) = part.split_once('=') {
                            if k.eq_ignore_ascii_case("media") || k.eq_ignore_ascii_case("pagesize")
                            {
                                return Some(v.to_owned());
                            }
                        }
                        None
                    })
                } else {
                    None
                }
            }),
    };

    let Some(raw) = raw_name else {
        return Ok(None);
    };

    let clean = raw.strip_suffix(".Borderless").unwrap_or(&raw);
    let Some(spec) = media_spec(clean) else {
        return Ok(None);
    };
    if spec.keyword == "custom" {
        return Ok(None);
    }
    Ok(TestMedia::from_keyword(spec.keyword))
}

pub(crate) fn submit_blocking(
    endpoint: &CupsServerEndpoint,
    proxy_authority: &str,
    target: &RecognisedClientQueue,
    media: TestMedia,
) -> SubmissionOutcome {
    use SubmissionOutcome::*;

    let cups_name = target.queue_name().as_str();

    // 1. Recheck default media
    let current_media = match read_default_media(endpoint, cups_name) {
        Ok(Some(m)) => m,
        _ => return Rejected,
    };
    if current_media != media {
        return Rejected;
    }

    // 2. Recheck destination existence and state
    let dests = match query_cups_destinations(endpoint) {
        Ok(d) => d,
        Err(_) => return Rejected,
    };
    let Some(dest) = dests.iter().find(|d| d.name == cups_name) else {
        return Rejected;
    };
    if !dest.accepting_jobs || dest.state == Some(IPP_PSTATE_STOPPED) {
        return Rejected;
    }

    // 3. Recheck destination URI matching the client queue target
    let record = SpoolerRecord::new(&dest.name, &dest.device_uri);
    match record.classify(proxy_authority) {
        SpoolerRecordClassification::RecognisedClientQueue(q) => {
            if q.server_address() != target.server_address()
                || q.printer_name() != target.printer_name()
            {
                return Rejected;
            }
        }
        _ => return Rejected,
    }

    // 4. Recheck active jobs in CUPS
    match query_cups_active_jobs(endpoint, cups_name) {
        Ok(0) => {}
        _ => return Rejected,
    }

    // 5. Prepare print settings
    let settings = PrintSettings {
        media: Some(media.keyword().to_owned()),
        copies: Some(1),
        color: Some(false),
        duplex: Some(DuplexMode::Simplex),
        orientation: Some(PrintOrientation::Portrait),
    };
    let option_pairs = match build_cups_job_options(&settings) {
        Ok(pairs) => pairs,
        Err(_) => return Rejected,
    };

    // 6. Generate diagnostic page PWG raster
    let document = generate_diagnostic_pwg_raster(media);
    if validate_pwg_raster_stream(&document).is_err() {
        return Rejected;
    }

    // 7. Connect to CUPS
    let conn = match CupsHttpConnection::connect(endpoint) {
        Ok(c) => c,
        Err(_) => return Rejected,
    };

    let Ok(dest_c) = CString::new(cups_name) else {
        return Rejected;
    };
    let title_c = c"ShaPrint diagnostic page";
    let docname_c = c"ShaPrint diagnostic document";
    let format_c = c"image/pwg-raster";

    let mut cups_options: *mut CupsOption = std::ptr::null_mut();
    let mut num_options: std::ffi::c_int = 0;

    unsafe {
        for (k, v) in &option_pairs {
            if let (Ok(k_c), Ok(v_c)) = (CString::new(k.as_str()), CString::new(v.as_str())) {
                num_options = (conn.lib.cups_add_option)(
                    k_c.as_ptr(),
                    v_c.as_ptr(),
                    num_options,
                    &mut cups_options,
                );
            }
        }
    }

    struct OptionsGuard {
        lib: &'static LibCups,
        num_options: std::ffi::c_int,
        options: *mut CupsOption,
    }
    impl Drop for OptionsGuard {
        fn drop(&mut self) {
            if !self.options.is_null() && self.num_options > 0 {
                unsafe { (self.lib.cups_free_options)(self.num_options, self.options) };
            }
        }
    }
    let _guard = OptionsGuard {
        lib: conn.lib,
        num_options,
        options: cups_options,
    };

    // 8. Create Job in CUPS
    let job_id = unsafe {
        (conn.lib.cups_create_job)(
            conn.http,
            dest_c.as_ptr(),
            title_c.as_ptr(),
            num_options,
            cups_options,
        )
    };
    if job_id <= 0 {
        return Rejected;
    }

    // Beyond this point, a job has been created in CUPS. Any failure is Uncertain.
    let start_status = unsafe {
        (conn.lib.cups_start_document)(
            conn.http,
            dest_c.as_ptr(),
            job_id,
            docname_c.as_ptr(),
            format_c.as_ptr(),
            1,
        )
    };
    if start_status != HTTP_STATUS_CONTINUE {
        unsafe {
            let _ = (conn.lib.cups_cancel_job2)(conn.http, dest_c.as_ptr(), job_id, 1);
        }
        return Uncertain;
    }

    for chunk in document.chunks(STREAM_CHUNK_BYTES) {
        let write_status = unsafe {
            (conn.lib.cups_write_request_data)(
                conn.http,
                chunk.as_ptr().cast::<std::ffi::c_char>(),
                chunk.len(),
            )
        };
        if write_status != HTTP_STATUS_CONTINUE {
            unsafe {
                let _ = (conn.lib.cups_cancel_job2)(conn.http, dest_c.as_ptr(), job_id, 1);
            }
            return Uncertain;
        }
    }

    let finish_status = unsafe { (conn.lib.cups_finish_document)(conn.http, dest_c.as_ptr()) };
    if !matches!(
        finish_status,
        IPP_STATUS_OK | IPP_STATUS_OK_SUBST | IPP_STATUS_OK_CONFLICT
    ) {
        unsafe {
            let _ = (conn.lib.cups_cancel_job2)(conn.http, dest_c.as_ptr(), job_id, 1);
        }
        return Uncertain;
    }

    Accepted
}

/// Linux native test print adapter operating on CUPS Client queues.
pub struct LinuxNativeTestPrint {
    endpoint: Option<CupsServerEndpoint>,
    proxy_authority: String,
}

impl Default for LinuxNativeTestPrint {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxNativeTestPrint {
    pub fn new() -> Self {
        Self {
            endpoint: None,
            proxy_authority: format!("127.0.0.1:{}", crate::adapters::CLIENT_PROXY_DEFAULT_PORT),
        }
    }

    pub fn with_authority(proxy_authority: impl Into<String>) -> Self {
        Self {
            endpoint: None,
            proxy_authority: proxy_authority.into(),
        }
    }

    pub fn with_endpoint_and_authority(
        endpoint: CupsServerEndpoint,
        proxy_authority: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: Some(endpoint),
            proxy_authority: proxy_authority.into(),
        }
    }

    fn resolve_endpoint(&self) -> CupsServerEndpoint {
        CupsServerEndpoint::resolve(self.endpoint.as_ref())
    }
}

#[async_trait::async_trait]
impl NativeTestPrint for LinuxNativeTestPrint {
    async fn default_media(
        &self,
        target: &RecognisedClientQueue,
    ) -> Result<Option<TestMedia>, AppError> {
        let endpoint = self.resolve_endpoint();
        let name = target.queue_name().to_string();
        tokio::task::spawn_blocking(move || read_default_media(&endpoint, &name))
            .await
            .map_err(|_| AppError::queue_unavailable("Requested media could not be read."))?
    }

    async fn submit(&self, target: &RecognisedClientQueue, media: TestMedia) -> SubmissionOutcome {
        let endpoint = self.resolve_endpoint();
        let authority = self.proxy_authority.clone();
        let target = target.clone();
        tokio::task::spawn_blocking(move || submit_blocking(&endpoint, &authority, &target, media))
            .await
            .unwrap_or(SubmissionOutcome::Uncertain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::printers::raster::decode_pwg;

    #[test]
    fn generates_valid_pwg_raster_diagnostic_page_for_a4() {
        let media = TestMedia::from_keyword("iso_a4_210x297mm").expect("A4 in catalog");
        let doc = generate_diagnostic_pwg_raster(media);
        validate_pwg_raster_stream(&doc).expect("valid PWG raster stream");

        let pages = decode_pwg(&doc).expect("decodes PWG raster");
        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        assert_eq!(page.width, 2480);
        assert_eq!(page.height, 3508);
        assert_eq!(page.dpi, [300, 300]);

        // Background should be predominantly white
        let non_white_count = page.pixels.chunks(4).filter(|p| p[0] != 255).count();
        assert!(non_white_count > 1000, "page must have drawn marks");

        // Center crosshair intersection should be black
        let cx = page.width as usize / 2;
        let cy = page.height as usize / 2;
        let center_idx = (cy * page.width as usize + cx) * 4;
        assert_eq!(page.pixels[center_idx], 0, "center pixel must be black");
    }

    #[test]
    fn generates_valid_pwg_raster_diagnostic_page_for_letter() {
        let media = TestMedia::from_keyword("na_letter_8.5x11in").expect("Letter in catalog");
        let doc = generate_diagnostic_pwg_raster(media);
        validate_pwg_raster_stream(&doc).expect("valid PWG raster stream");

        let pages = decode_pwg(&doc).expect("decodes PWG raster");
        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        assert_eq!(page.width, 2550);
        assert_eq!(page.height, 3300);
        assert_eq!(page.dpi, [300, 300]);
    }
}
