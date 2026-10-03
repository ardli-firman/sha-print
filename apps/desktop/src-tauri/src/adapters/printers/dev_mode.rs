//! Maps common job settings into the Windows driver's public DEVMODE fields.
use crate::domain::AppError;

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
        fields |= 0x0000_0001;
    }
    if let Some(media) = &settings.media {
        let paper = match media.as_str() {
            "na_letter_8.5x11in" => 1i16,
            "na_legal_8.5x14in" => 5i16,
            "iso_a3_297x420mm" => 8i16,
            "iso_a4_210x297mm" => 9i16,
            "iso_a5_148x210mm" => 11i16,
            _ => {
                return Err(AppError::invalid_input(
                    "the selected media size is not supported by the Windows adapter",
                ))
            }
        };
        // DEVMODEW.dmOrientation is at byte 76; dmPaperSize is at byte 78.
        dev_mode[78..80].copy_from_slice(&paper.to_ne_bytes());
        // A custom form's dimensions override dmPaperSize. Clear the queue's old form
        // before selecting the client's standard media, including inherited F4 defaults.
        fields &= !(0x0000_0004 | 0x0000_0008 | 0x0001_0000);
        dev_mode[80..84].fill(0);
        if let Some(form_name) = dev_mode.get_mut(102..166) {
            form_name.fill(0);
        }
        fields |= 0x0000_0002;
    }
    if let Some(copies) = settings.copies {
        if !(1..=999).contains(&copies) {
            return Err(AppError::invalid_input(
                "the requested copy count is outside the supported range",
            ));
        }
        dev_mode[86..88].copy_from_slice(&(copies as i16).to_ne_bytes());
        fields |= 0x0000_0100;
    }
    if let Some(color) = settings.color {
        dev_mode[92..94].copy_from_slice(&(if color { 2i16 } else { 1i16 }).to_ne_bytes());
        fields |= 0x0000_0800;
    }
    if let Some(duplex) = settings.duplex {
        let mode = match duplex {
            crate::application::DuplexMode::Simplex => 1i16,
            crate::application::DuplexMode::LongEdge => 2i16,
            crate::application::DuplexMode::ShortEdge => 3i16,
        };
        dev_mode[94..96].copy_from_slice(&mode.to_ne_bytes());
        fields |= 0x0000_1000;
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
    fn an_a4_job_replaces_an_inherited_f4_custom_form() {
        let mut devmode = vec![0u8; 220];
        // Queue default: F4 custom form, 210 x 330 mm. Public length/width override paper enum.
        let custom_fields = 0x0000_0004u32 | 0x0000_0008 | 0x0001_0000;
        devmode[72..76].copy_from_slice(&custom_fields.to_ne_bytes());
        devmode[80..82].copy_from_slice(&3300i16.to_ne_bytes());
        devmode[82..84].copy_from_slice(&2100i16.to_ne_bytes());
        devmode[102..108].copy_from_slice(&[b'F', 0, b'4', 0, 0, 0]);
        let settings = crate::application::PrintSettings {
            media: Some("iso_a4_210x297mm".to_owned()),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies A4");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(
            fields & custom_fields,
            0,
            "inherited F4 fields still override the client's A4 page"
        );
        assert_eq!(i16::from_ne_bytes(devmode[78..80].try_into().unwrap()), 9);
        assert_eq!(&devmode[80..84], &[0; 4]);
        assert_eq!(&devmode[102..166], &[0; 64]);
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
