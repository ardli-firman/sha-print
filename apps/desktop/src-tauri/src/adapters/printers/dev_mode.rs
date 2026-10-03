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
        "na_foolscap_8.5x13in" => Ok(MediaSpec {
            paper_id: 14,
            short_tenths_mm: 2159,
            long_tenths_mm: 3302,
            explicit_dimensions: false,
        }),
        "om_folio_210x330mm" => Ok(MediaSpec {
            paper_id: 14,
            short_tenths_mm: 2100,
            long_tenths_mm: 3300,
            explicit_dimensions: true,
        }),
        _ => Err(AppError::invalid_input(
            "the selected media size is not supported by the Windows adapter",
        )),
    }
}

fn paper_id_dimensions_tenths_mm(paper_id: i16) -> Option<(i16, i16)> {
    match paper_id {
        1 => Some((2159, 2794)),
        5 => Some((2159, 3556)),
        8 => Some((2970, 4200)),
        9 => Some((2100, 2970)),
        11 => Some((1480, 2100)),
        14 => Some((2159, 3302)),
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
    loaded.0 + TOLERANCE_TENTHS_MM >= requested.short_tenths_mm
        && loaded.1 + TOLERANCE_TENTHS_MM >= requested.long_tenths_mm
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
}
