//! Resolve page geometry when a native IPP client omits job-level media or orientation.
use super::raster::RasterPage;
use crate::application::{PrintOrientation, PrintSettings};

pub(super) fn settings_for_page(requested: &PrintSettings, page: &RasterPage) -> PrintSettings {
    let mut resolved = requested.clone();
    let width_mm = f64::from(page.width) * 25.4 / f64::from(page.dpi[0]);
    let height_mm = f64::from(page.height) * 25.4 / f64::from(page.dpi[1]);
    if resolved.orientation.is_none() {
        resolved.orientation = Some(if width_mm > height_mm {
            PrintOrientation::Landscape
        } else {
            PrintOrientation::Portrait
        });
    }
    if resolved.media.is_none() {
        let short_edge = width_mm.min(height_mm);
        let long_edge = width_mm.max(height_mm);
        // Raster dimensions are whole pixels, so allow the rounding of millimeters to pixels.
        for (media, width, height) in [
            ("iso_a4_210x297mm", 210.0, 297.0),
            ("na_letter_8.5x11in", 215.9, 279.4),
            ("na_legal_8.5x14in", 215.9, 355.6),
            ("iso_a3_297x420mm", 297.0, 420.0),
            ("iso_a5_148x210mm", 148.0, 210.0),
            ("iso_a6_105x148mm", 105.0, 148.0),
            ("iso_b4_250x353mm", 250.0, 353.0),
            ("iso_b5_176x250mm", 176.0, 250.0),
            ("jis_b5_182x257mm", 182.0, 257.0),
            ("om_folio_210x330mm", 210.0, 330.0),
            ("na_foolscap_8.5x13in", 215.9, 330.2),
            ("na_executive_7.25x10.5in", 184.2, 266.7),
            ("na_invoice_5.5x8.5in", 139.7, 215.9),
            ("na_ledger_11x17in", 279.4, 431.8),
            ("oe_photo-4x6_4x6in", 101.6, 152.4),
            ("na_5x7_5x7in", 127.0, 177.8),
            ("iso_dl_110x220mm", 110.0, 220.0),
            ("iso_c5_162x229mm", 162.0, 229.0),
            ("na_number-10_4.125x9.5in", 104.8, 241.3),
            ("na_monarch_3.875x7.5in", 98.4, 190.5),
        ] {
            if (short_edge - width).abs() <= 0.5 && (long_edge - height).abs() <= 0.5 {
                resolved.media = Some(media.to_owned());
                break;
            }
        }
        if resolved.media.is_none() {
            resolved.media = Some(format!("custom_{:.1}x{:.1}mm", short_edge, long_edge));
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::PrintOrientation;

    #[test]
    fn a4_raster_without_job_media_uses_a4_portrait_instead_of_server_defaults() {
        let page = RasterPage {
            width: 2480,
            height: 3508,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved = settings_for_page(&PrintSettings::default(), &page);
        assert_eq!(resolved.media.as_deref(), Some("iso_a4_210x297mm"));
        assert_eq!(resolved.orientation, Some(PrintOrientation::Portrait));
    }

    #[test]
    fn landscape_page_geometry_is_used_when_client_omits_orientation() {
        let page = RasterPage {
            width: 3508,
            height: 2480,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved = settings_for_page(&PrintSettings::default(), &page);
        assert_eq!(resolved.media.as_deref(), Some("iso_a4_210x297mm"));
        assert_eq!(resolved.orientation, Some(PrintOrientation::Landscape));
    }

    #[test]
    fn explicit_client_settings_take_precedence_over_raster_geometry() {
        let page = RasterPage {
            width: 3508,
            height: 2480,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let requested = PrintSettings {
            media: Some("na_letter_8.5x11in".to_owned()),
            orientation: Some(PrintOrientation::Portrait),
            copies: Some(2),
            color: Some(false),
            duplex: Some(crate::application::DuplexMode::Simplex),
        };
        assert_eq!(settings_for_page(&requested, &page), requested);
    }

    #[test]
    fn unknown_raster_size_is_not_relabelled_as_a4() {
        let page = RasterPage {
            width: 2400,
            height: 3900,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved = settings_for_page(&PrintSettings::default(), &page);
        assert_ne!(resolved.media.as_deref(), Some("iso_a4_210x297mm"));
        assert_eq!(resolved.media.as_deref(), Some("custom_203.2x330.2mm"));
    }

    #[test]
    fn orientation_uses_physical_dimensions_when_dpi_differs_between_axes() {
        let page = RasterPage {
            width: 4961,
            height: 3508,
            dpi: [600, 300],
            pixels: Vec::new(),
        };
        let resolved = settings_for_page(&PrintSettings::default(), &page);
        assert_eq!(resolved.media.as_deref(), Some("iso_a4_210x297mm"));
        assert_eq!(resolved.orientation, Some(PrintOrientation::Portrait));
    }

    #[test]
    fn f4_and_folio_rasters_without_job_media_resolve_to_their_media_names() {
        let f4_page = RasterPage {
            width: 2480,
            height: 3898,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved_f4 = settings_for_page(&PrintSettings::default(), &f4_page);
        assert_eq!(resolved_f4.media.as_deref(), Some("om_folio_210x330mm"));
        assert_eq!(resolved_f4.orientation, Some(PrintOrientation::Portrait));

        let folio_page = RasterPage {
            width: 3900,
            height: 2550,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved_folio = settings_for_page(&PrintSettings::default(), &folio_page);
        assert_eq!(
            resolved_folio.media.as_deref(),
            Some("na_foolscap_8.5x13in")
        );
        assert_eq!(
            resolved_folio.orientation,
            Some(PrintOrientation::Landscape)
        );
    }

    #[test]
    fn a6_and_photo_sizes_without_job_media_resolve_correctly() {
        let a6_page = RasterPage {
            width: 1240,
            height: 1748,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved_a6 = settings_for_page(&PrintSettings::default(), &a6_page);
        assert_eq!(resolved_a6.media.as_deref(), Some("iso_a6_105x148mm"));

        let photo_page = RasterPage {
            width: 1200,
            height: 1800,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved_photo = settings_for_page(&PrintSettings::default(), &photo_page);
        assert_eq!(resolved_photo.media.as_deref(), Some("oe_photo-4x6_4x6in"));
    }
}
