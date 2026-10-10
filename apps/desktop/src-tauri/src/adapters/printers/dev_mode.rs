//! Maps common job settings into the Windows driver's public DEVMODE fields.
use crate::domain::AppError;

const DM_ORIENTATION: u32 = 0x0000_0001;
const DM_PAPERSIZE: u32 = 0x0000_0002;
const DM_PAPERLENGTH: u32 = 0x0000_0004;
const DM_PAPERWIDTH: u32 = 0x0000_0008;
const DM_COPIES: u32 = 0x0000_0100;
const DM_COLOR: u32 = 0x0000_0800;
const DM_DUPLEX: u32 = 0x0000_1000;
const DM_FORMNAME: u32 = 0x0001_0000;

struct MediaSpec {
    paper_id: i16,
    short_tenths_mm: i16,
    long_tenths_mm: i16,
    explicit_dimensions: bool,
}

fn requested_media_spec(media: &str) -> Result<MediaSpec, AppError> {
    match media {
        "na_letter_8.5x11in" => Ok(MediaSpec {
            paper_id: 1,
            short_tenths_mm: 2159,
            long_tenths_mm: 2794,
            explicit_dimensions: false,
        }),
        "na_legal_8.5x14in" => Ok(MediaSpec {
            paper_id: 5,
            short_tenths_mm: 2159,
            long_tenths_mm: 3556,
            explicit_dimensions: false,
        }),
        "na_ledger_11x17in" | "na_tabloid_11x17in" => Ok(MediaSpec {
            paper_id: 3,
            short_tenths_mm: 2794,
            long_tenths_mm: 4318,
            explicit_dimensions: false,
        }),
        "na_invoice_5.5x8.5in" => Ok(MediaSpec {
            paper_id: 6,
            short_tenths_mm: 1397,
            long_tenths_mm: 2159,
            explicit_dimensions: false,
        }),
        "na_executive_7.25x10.5in" => Ok(MediaSpec {
            paper_id: 7,
            short_tenths_mm: 1842,
            long_tenths_mm: 2667,
            explicit_dimensions: false,
        }),
        "iso_a3_297x420mm" => Ok(MediaSpec {
            paper_id: 8,
            short_tenths_mm: 2970,
            long_tenths_mm: 4200,
            explicit_dimensions: false,
        }),
        "iso_a4_210x297mm" => Ok(MediaSpec {
            paper_id: 9,
            short_tenths_mm: 2100,
            long_tenths_mm: 2970,
            explicit_dimensions: false,
        }),
        "iso_a5_148x210mm" => Ok(MediaSpec {
            paper_id: 11,
            short_tenths_mm: 1480,
            long_tenths_mm: 2100,
            explicit_dimensions: false,
        }),
        "iso_a6_105x148mm" => Ok(MediaSpec {
            paper_id: 70,
            short_tenths_mm: 1050,
            long_tenths_mm: 1480,
            explicit_dimensions: false,
        }),
        "iso_b4_250x353mm" => Ok(MediaSpec {
            paper_id: 12,
            short_tenths_mm: 2500,
            long_tenths_mm: 3530,
            explicit_dimensions: true,
        }),
        "iso_b5_176x250mm" => Ok(MediaSpec {
            paper_id: 13,
            short_tenths_mm: 1760,
            long_tenths_mm: 2500,
            explicit_dimensions: true,
        }),
        "jis_b5_182x257mm" => Ok(MediaSpec {
            paper_id: 13,
            short_tenths_mm: 1820,
            long_tenths_mm: 2570,
            explicit_dimensions: false,
        }),
        "na_foolscap_8.5x13in" => Ok(MediaSpec {
            paper_id: 14,
            short_tenths_mm: 2159,
            long_tenths_mm: 3302,
            explicit_dimensions: false,
        }),
        "om_folio_210x330mm" | "f4" | "F4" | "folio" | "Folio" => Ok(MediaSpec {
            paper_id: 14,
            short_tenths_mm: 2100,
            long_tenths_mm: 3300,
            explicit_dimensions: true,
        }),
        "oe_photo-4x6_4x6in" | "na_index-4x6_4x6in" => Ok(MediaSpec {
            paper_id: 256,
            short_tenths_mm: 1016,
            long_tenths_mm: 1524,
            explicit_dimensions: true,
        }),
        "na_5x7_5x7in" => Ok(MediaSpec {
            paper_id: 256,
            short_tenths_mm: 1270,
            long_tenths_mm: 1778,
            explicit_dimensions: true,
        }),
        "iso_dl_110x220mm" => Ok(MediaSpec {
            paper_id: 27,
            short_tenths_mm: 1100,
            long_tenths_mm: 2200,
            explicit_dimensions: false,
        }),
        "iso_c5_162x229mm" => Ok(MediaSpec {
            paper_id: 28,
            short_tenths_mm: 1620,
            long_tenths_mm: 2290,
            explicit_dimensions: false,
        }),
        "na_number-10_4.125x9.5in" => Ok(MediaSpec {
            paper_id: 20,
            short_tenths_mm: 1048,
            long_tenths_mm: 2413,
            explicit_dimensions: false,
        }),
        "na_monarch_3.875x7.5in" => Ok(MediaSpec {
            paper_id: 37,
            short_tenths_mm: 984,
            long_tenths_mm: 1905,
            explicit_dimensions: false,
        }),
        other => {
            if let Some((short, long)) = parse_media_dimensions(other) {
                let (paper_id, explicit) =
                    match_standard_paper_id(short, long).unwrap_or((256, true));
                Ok(MediaSpec {
                    paper_id,
                    short_tenths_mm: short,
                    long_tenths_mm: long,
                    explicit_dimensions: explicit,
                })
            } else {
                Err(AppError::invalid_input(
                    "the selected media size is not supported by the Windows adapter",
                ))
            }
        }
    }
}

fn parse_media_dimensions(media: &str) -> Option<(i16, i16)> {
    let (dim_str, scale) = if let Some(s) = media.strip_suffix("mm") {
        let s = s.strip_suffix("_100th-").unwrap_or(s);
        (s, 10.0)
    } else if let Some(s) = media.strip_suffix("cm") {
        (s, 100.0)
    } else {
        let s = media.strip_suffix("in")?;
        (s, 254.0)
    };

    let candidate = dim_str.rsplit('_').next().unwrap_or(dim_str);
    let (w_str, h_str) = if let Some((w, h)) = candidate.split_once('x') {
        (w.trim_start_matches(|c: char| !c.is_ascii_digit()), h)
    } else {
        let h_idx = candidate.find('h')?;
        let (w, h) = candidate.split_at(h_idx);
        (w.trim_start_matches(|c: char| !c.is_ascii_digit()), &h[1..])
    };

    let w = w_str.parse::<f64>().ok()?;
    let h = h_str.parse::<f64>().ok()?;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }

    let d1 = (w * scale).round() as i16;
    let d2 = (h * scale).round() as i16;
    if d1 <= 0 || d2 <= 0 {
        return None;
    }

    Some((d1.min(d2), d1.max(d2)))
}

fn match_standard_paper_id(short: i16, long: i16) -> Option<(i16, bool)> {
    const TOLERANCE: i16 = 5;
    for (id, s, l, explicit) in [
        (9, 2100, 2970, false),  // A4
        (1, 2159, 2794, false),  // Letter
        (5, 2159, 3556, false),  // Legal
        (14, 2100, 3300, true),  // Folio F4
        (14, 2159, 3302, false), // Foolscap
        (8, 2970, 4200, false),  // A3
        (11, 1480, 2100, false), // A5
        (70, 1050, 1480, false), // A6
        (13, 1760, 2500, true),  // ISO B5
        (13, 1820, 2570, false), // JIS B5
        (12, 2500, 3530, true),  // ISO B4
        (7, 1842, 2667, false),  // Executive
        (6, 1397, 2159, false),  // Statement
        (3, 2794, 4318, false),  // Ledger / Tabloid
        (27, 1100, 2200, false), // DL Envelope
        (28, 1620, 2290, false), // C5 Envelope
        (20, 1048, 2413, false), // #10 Envelope
        (37, 984, 1905, false),  // Monarch Envelope
    ] {
        if (short - s).abs() <= TOLERANCE && (long - l).abs() <= TOLERANCE {
            return Some((id, explicit));
        }
    }
    None
}

fn paper_id_dimensions_tenths_mm(paper_id: i16) -> Option<(i16, i16)> {
    match paper_id {
        1 => Some((2159, 2794)),
        3 => Some((2794, 4318)),
        5 => Some((2159, 3556)),
        6 => Some((1397, 2159)),
        7 => Some((1842, 2667)),
        8 => Some((2970, 4200)),
        9 => Some((2100, 2970)),
        11 => Some((1480, 2100)),
        12 => Some((2500, 3530)),
        13 => Some((1820, 2570)),
        14 => Some((2159, 3302)),
        20 => Some((1048, 2413)),
        27 => Some((1100, 2200)),
        28 => Some((1620, 2290)),
        37 => Some((984, 1905)),
        70 => Some((1050, 1480)),
        _ => None,
    }
}

fn loaded_media_dimensions_tenths_mm(dev_mode: &[u8], fields: u32) -> Option<(i16, i16)> {
    if fields & (DM_PAPERLENGTH | DM_PAPERWIDTH) == (DM_PAPERLENGTH | DM_PAPERWIDTH) {
        let length = i16::from_ne_bytes(dev_mode[80..82].try_into().ok()?);
        let width = i16::from_ne_bytes(dev_mode[82..84].try_into().ok()?);
        if length > 0 && width > 0 {
            return Some((width.min(length), width.max(length)));
        }
    }
    if fields & DM_PAPERSIZE != 0 {
        let paper_id = i16::from_ne_bytes(dev_mode[78..80].try_into().ok()?);
        return paper_id_dimensions_tenths_mm(paper_id);
    }
    None
}

fn loaded_media_fits_requested(loaded: (i16, i16), requested: &MediaSpec) -> bool {
    const TOLERANCE_TENTHS_MM: i16 = 5;
    // ADR 0010: Preserve a loaded Folio/F4 sheet when an A4 document is printed onto it,
    // so physical inkjet feed length and landscape coordinate origins match the physical sheet.
    let is_a4_requested = (requested.short_tenths_mm - 2100).abs() <= TOLERANCE_TENTHS_MM
        && (requested.long_tenths_mm - 2970).abs() <= TOLERANCE_TENTHS_MM;
    let is_folio_loaded = (loaded.0 - 2100).abs() <= 60 && loaded.1 >= 3290;
    is_a4_requested && is_folio_loaded
}

pub(super) fn apply_settings_to_dev_mode(
    dev_mode: &mut [u8],
    settings: &crate::application::PrintSettings,
) -> Result<(), AppError> {
    if dev_mode.len() < 96 {
        return Err(AppError::internal(
            "invalid Windows printer settings buffer",
        ));
    }
    let mut fields = u32::from_ne_bytes(
        dev_mode[72..76]
            .try_into()
            .map_err(|_| AppError::internal("invalid Windows printer settings"))?,
    );
    if let Some(orientation) = settings.orientation {
        let dm_orient = match orientation {
            crate::application::PrintOrientation::Portrait => 1i16,
            crate::application::PrintOrientation::Landscape => 2i16,
        };
        // DEVMODEW.dmOrientation is at byte 76; DM_ORIENTATION is 0x0000_0001
        dev_mode[76..78].copy_from_slice(&dm_orient.to_ne_bytes());
        fields |= DM_ORIENTATION;
    }
    if let Some(media) = &settings.media {
        let spec = requested_media_spec(media.as_str())?;
        let keep_loaded = loaded_media_dimensions_tenths_mm(dev_mode, fields)
            .is_some_and(|loaded| loaded_media_fits_requested(loaded, &spec));
        if !keep_loaded {
            // DEVMODEW.dmOrientation is at byte 76; dmPaperSize is at byte 78.
            dev_mode[78..80].copy_from_slice(&spec.paper_id.to_ne_bytes());
            if let Some(form_name) = dev_mode.get_mut(102..166) {
                form_name.fill(0);
            }
            fields &= !DM_FORMNAME;
            if spec.explicit_dimensions {
                dev_mode[80..82].copy_from_slice(&spec.long_tenths_mm.to_ne_bytes());
                dev_mode[82..84].copy_from_slice(&spec.short_tenths_mm.to_ne_bytes());
                fields |= DM_PAPERSIZE | DM_PAPERLENGTH | DM_PAPERWIDTH;
            } else {
                fields &= !(DM_PAPERLENGTH | DM_PAPERWIDTH);
                dev_mode[80..84].fill(0);
                fields |= DM_PAPERSIZE;
            }
        }
    }
    if let Some(copies) = settings.copies {
        if !(1..=999).contains(&copies) {
            return Err(AppError::invalid_input(
                "the requested copy count is outside the supported range",
            ));
        }
        dev_mode[86..88].copy_from_slice(&(copies as i16).to_ne_bytes());
        fields |= DM_COPIES;
    }
    if let Some(color) = settings.color {
        dev_mode[92..94].copy_from_slice(&(if color { 2i16 } else { 1i16 }).to_ne_bytes());
        fields |= DM_COLOR;
    }
    if let Some(duplex) = settings.duplex {
        let mode = match duplex {
            crate::application::DuplexMode::Simplex => 1i16,
            crate::application::DuplexMode::LongEdge => 2i16,
            crate::application::DuplexMode::ShortEdge => 3i16,
        };
        dev_mode[94..96].copy_from_slice(&mode.to_ne_bytes());
        fields |= DM_DUPLEX;
    }
    dev_mode[72..76].copy_from_slice(&fields.to_ne_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dev_mode_mapping_applies_orientation_and_media_size() {
        use crate::application::PrintOrientation;

        let mut devmode = vec![0u8; 120];
        let settings = crate::application::PrintSettings {
            media: Some("na_legal_8.5x14in".to_string()),
            orientation: Some(PrintOrientation::Landscape),
            copies: Some(3),
            color: Some(false),
            duplex: Some(crate::application::DuplexMode::LongEdge),
        };

        apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");

        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        // DM_ORIENTATION = 0x0000_0001
        // DM_PAPERSIZE   = 0x0000_0002
        // DM_COPIES      = 0x0000_0100
        // DM_COLOR       = 0x0000_0800
        // DM_DUPLEX      = 0x0000_1000
        let expected_fields = 0x0000_0001 | 0x0000_0002 | 0x0000_0100 | 0x0000_0800 | 0x0000_1000;
        assert_eq!(fields, expected_fields);

        // dmOrientation at 76..78 (2 = DMORIENT_LANDSCAPE)
        let orientation = i16::from_ne_bytes(devmode[76..78].try_into().unwrap());
        assert_eq!(orientation, 2);

        // dmPaperSize at 78..80 (5 = DMPAPER_LEGAL)
        let paper = i16::from_ne_bytes(devmode[78..80].try_into().unwrap());
        assert_eq!(paper, 5);
    }

    #[test]
    fn dev_mode_mapping_applies_color_modes() {
        for (color, expected_dm) in [(true, 2i16), (false, 1i16)] {
            let mut devmode = vec![0u8; 120];
            let settings = crate::application::PrintSettings {
                color: Some(color),
                ..Default::default()
            };
            apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");
            let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
            assert_eq!(fields, 0x0000_0800);
            let dm_color = i16::from_ne_bytes(devmode[92..94].try_into().unwrap());
            assert_eq!(dm_color, expected_dm, "color: {color}");
        }
    }

    #[test]
    fn dev_mode_mapping_applies_duplex_modes() {
        use crate::application::DuplexMode;
        for (duplex, expected_dm) in [
            (DuplexMode::Simplex, 1i16),
            (DuplexMode::LongEdge, 2i16),
            (DuplexMode::ShortEdge, 3i16),
        ] {
            let mut devmode = vec![0u8; 120];
            let settings = crate::application::PrintSettings {
                duplex: Some(duplex),
                ..Default::default()
            };
            apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");
            let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
            assert_eq!(fields, 0x0000_1000);
            let dm_duplex = i16::from_ne_bytes(devmode[94..96].try_into().unwrap());
            assert_eq!(dm_duplex, expected_dm, "duplex: {duplex:?}");
        }
    }

    #[test]
    fn dev_mode_mapping_applies_copies_and_rejects_out_of_range() {
        let mut devmode = vec![0u8; 120];
        let settings = crate::application::PrintSettings {
            copies: Some(7),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(fields, 0x0000_0100);
        let dm_copies = i16::from_ne_bytes(devmode[86..88].try_into().unwrap());
        assert_eq!(dm_copies, 7);

        for invalid in [0, 1000] {
            let mut devmode = vec![0u8; 120];
            let settings = crate::application::PrintSettings {
                copies: Some(invalid),
                ..Default::default()
            };
            assert!(
                apply_settings_to_dev_mode(&mut devmode, &settings).is_err(),
                "should reject copies: {invalid}"
            );
        }
    }
    #[test]
    fn an_a4_job_preserves_a_larger_loaded_f4_custom_form_and_applies_orientation() {
        use crate::application::PrintOrientation;

        let mut devmode = vec![0u8; 220];
        // Queue default: F4 custom form, 210 x 330 mm.
        let custom_fields = DM_PAPERLENGTH | DM_PAPERWIDTH | DM_FORMNAME;
        devmode[72..76].copy_from_slice(&custom_fields.to_ne_bytes());
        devmode[80..82].copy_from_slice(&3300i16.to_ne_bytes());
        devmode[82..84].copy_from_slice(&2100i16.to_ne_bytes());
        devmode[102..108].copy_from_slice(&[b'F', 0, b'4', 0, 0, 0]);
        let settings = crate::application::PrintSettings {
            media: Some("iso_a4_210x297mm".to_owned()),
            orientation: Some(PrintOrientation::Landscape),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(fields, custom_fields | DM_ORIENTATION);
        assert_eq!(i16::from_ne_bytes(devmode[76..78].try_into().unwrap()), 2);
        assert_eq!(
            i16::from_ne_bytes(devmode[80..82].try_into().unwrap()),
            3300
        );
        assert_eq!(
            i16::from_ne_bytes(devmode[82..84].try_into().unwrap()),
            2100
        );
        assert_eq!(&devmode[102..108], &[b'F', 0, b'4', 0, 0, 0]);
    }

    #[test]
    fn an_a4_job_preserves_a_larger_loaded_standard_paper_size() {
        let mut devmode = vec![0u8; 220];
        // Queue default: DMPAPER_FOLIO (14), 8.5 x 13 in (215.9 x 330.2 mm).
        devmode[72..76].copy_from_slice(&DM_PAPERSIZE.to_ne_bytes());
        devmode[78..80].copy_from_slice(&14i16.to_ne_bytes());
        let settings = crate::application::PrintSettings {
            media: Some("iso_a4_210x297mm".to_owned()),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut devmode, &settings).expect("keeps loaded Folio");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(fields, DM_PAPERSIZE);
        assert_eq!(i16::from_ne_bytes(devmode[78..80].try_into().unwrap()), 14);
    }

    #[test]
    fn a_requested_media_larger_than_loaded_custom_form_replaces_it() {
        let mut devmode = vec![0u8; 220];
        // Queue default: custom form 148 x 210 mm (A5).
        let custom_fields = DM_PAPERLENGTH | DM_PAPERWIDTH | DM_FORMNAME;
        devmode[72..76].copy_from_slice(&custom_fields.to_ne_bytes());
        devmode[80..82].copy_from_slice(&2100i16.to_ne_bytes());
        devmode[82..84].copy_from_slice(&1480i16.to_ne_bytes());
        devmode[102..108].copy_from_slice(&[b'A', 0, b'5', 0, 0, 0]);
        let settings = crate::application::PrintSettings {
            media: Some("iso_a4_210x297mm".to_owned()),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut devmode, &settings).expect("replaces smaller form with A4");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(fields & custom_fields, 0);
        assert_eq!(fields, DM_PAPERSIZE);
        assert_eq!(i16::from_ne_bytes(devmode[78..80].try_into().unwrap()), 9);
        assert_eq!(&devmode[80..84], &[0; 4]);
        assert_eq!(&devmode[102..166], &[0; 64]);
    }

    #[test]
    fn f4_and_folio_media_sizes_are_mapped_to_dev_mode() {
        let mut folio_devmode = vec![0u8; 220];
        let folio_settings = crate::application::PrintSettings {
            media: Some("na_foolscap_8.5x13in".to_owned()),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut folio_devmode, &folio_settings)
            .expect("applies na_foolscap_8.5x13in");
        let folio_fields = u32::from_ne_bytes(folio_devmode[72..76].try_into().unwrap());
        assert_eq!(folio_fields, DM_PAPERSIZE);
        assert_eq!(
            i16::from_ne_bytes(folio_devmode[78..80].try_into().unwrap()),
            14
        );

        let mut f4_devmode = vec![0u8; 220];
        let f4_settings = crate::application::PrintSettings {
            media: Some("om_folio_210x330mm".to_owned()),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut f4_devmode, &f4_settings)
            .expect("applies om_folio_210x330mm");
        let f4_fields = u32::from_ne_bytes(f4_devmode[72..76].try_into().unwrap());
        assert_eq!(f4_fields, DM_PAPERSIZE | DM_PAPERLENGTH | DM_PAPERWIDTH);
        assert_eq!(
            i16::from_ne_bytes(f4_devmode[78..80].try_into().unwrap()),
            14
        );
        assert_eq!(
            i16::from_ne_bytes(f4_devmode[80..82].try_into().unwrap()),
            3300
        );
        assert_eq!(
            i16::from_ne_bytes(f4_devmode[82..84].try_into().unwrap()),
            2100
        );
    }

    #[test]
    fn a_job_without_media_keeps_the_queues_custom_form() {
        let mut devmode = vec![0u8; 220];
        let fields = 0x0001_000cu32;
        devmode[72..76].copy_from_slice(&fields.to_ne_bytes());
        devmode[80..82].copy_from_slice(&3300i16.to_ne_bytes());
        devmode[82..84].copy_from_slice(&2100i16.to_ne_bytes());
        let before = devmode.clone();
        apply_settings_to_dev_mode(&mut devmode, &crate::application::PrintSettings::default())
            .expect("keeps defaults");
        assert_eq!(devmode, before);
    }

    #[test]
    fn custom_media_size_applies_dmpaper_user_and_dimensions() {
        for (media_name, expected_short, expected_long) in [
            ("custom_100x150mm", 1000i16, 1500i16),
            ("custom_min_100x150mm", 1000i16, 1500i16),
            ("custom_4x6in", 1016i16, 1524i16),
        ] {
            let mut devmode = vec![0u8; 220];
            let settings = crate::application::PrintSettings {
                media: Some(media_name.to_owned()),
                ..Default::default()
            };
            apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies custom media size");
            let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
            assert_eq!(fields, DM_PAPERSIZE | DM_PAPERLENGTH | DM_PAPERWIDTH);
            assert_eq!(
                i16::from_ne_bytes(devmode[78..80].try_into().unwrap()),
                256 // DMPAPER_USER
            );
            assert_eq!(
                i16::from_ne_bytes(devmode[80..82].try_into().unwrap()),
                expected_long
            );
            assert_eq!(
                i16::from_ne_bytes(devmode[82..84].try_into().unwrap()),
                expected_short
            );
        }
    }

    #[test]
    fn an_a5_job_overwrites_loaded_a4_queue_settings() {
        let mut devmode = vec![0u8; 220];
        // Queue default: A4 (DMPAPER_A4 = 9)
        devmode[72..76].copy_from_slice(&DM_PAPERSIZE.to_ne_bytes());
        devmode[78..80].copy_from_slice(&9i16.to_ne_bytes());
        let settings = crate::application::PrintSettings {
            media: Some("iso_a5_148x210mm".to_owned()),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut devmode, &settings)
            .expect("applies A5 instead of keeping A4");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(fields, DM_PAPERSIZE);
        assert_eq!(i16::from_ne_bytes(devmode[78..80].try_into().unwrap()), 11);
        // DMPAPER_A5
    }

    #[test]
    fn custom_paper_overwrites_loaded_a4_queue_settings() {
        let mut devmode = vec![0u8; 220];
        // Queue default: A4 (DMPAPER_A4 = 9)
        devmode[72..76].copy_from_slice(&DM_PAPERSIZE.to_ne_bytes());
        devmode[78..80].copy_from_slice(&9i16.to_ne_bytes());
        let settings = crate::application::PrintSettings {
            media: Some("custom_100x150mm".to_owned()),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut devmode, &settings)
            .expect("applies custom size instead of keeping A4");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(fields, DM_PAPERSIZE | DM_PAPERLENGTH | DM_PAPERWIDTH);
        assert_eq!(i16::from_ne_bytes(devmode[78..80].try_into().unwrap()), 256);
        assert_eq!(
            i16::from_ne_bytes(devmode[80..82].try_into().unwrap()),
            1500
        );
        assert_eq!(
            i16::from_ne_bytes(devmode[82..84].try_into().unwrap()),
            1000
        );
    }

    #[test]
    fn invalid_media_string_without_dimensions_is_rejected() {
        let mut devmode = vec![0u8; 120];
        let settings = crate::application::PrintSettings {
            media: Some("unsupported_media_format".to_owned()),
            ..Default::default()
        };
        assert!(apply_settings_to_dev_mode(&mut devmode, &settings).is_err());
    }
}
