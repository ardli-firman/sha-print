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
        ] {
            if (short_edge - width).abs() <= 0.5 && (long_edge - height).abs() <= 0.5 {
                resolved.media = Some(media.to_owned());
                break;
            }
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
        assert!(settings_for_page(&PrintSettings::default(), &page)
            .media
            .is_none());
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
}
