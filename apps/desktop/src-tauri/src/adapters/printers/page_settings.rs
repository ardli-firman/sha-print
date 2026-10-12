//! Resolve page geometry when a native IPP client omits job-level media or orientation.
use super::{
    media::{custom_dimensions_are_supported, media_for_dimensions, media_spec},
    raster::RasterPage,
};
use crate::application::{PrintOrientation, PrintSettings};
use crate::domain::AppError;

pub(super) fn settings_for_page(
    requested: &PrintSettings,
    page: &RasterPage,
) -> Result<PrintSettings, AppError> {
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
    if resolved
        .media
        .as_deref()
        .is_none_or(|media| media_spec(media).is_none())
    {
        let short_edge = width_mm.min(height_mm);
        let long_edge = width_mm.max(height_mm);
        let short_tenths_mm = (short_edge * 10.0).round() as i16;
        let long_tenths_mm = (long_edge * 10.0).round() as i16;
        resolved.media = if let Some(spec) = media_for_dimensions(short_tenths_mm, long_tenths_mm) {
            Some(spec.keyword.to_owned())
        } else if custom_dimensions_are_supported(short_tenths_mm, long_tenths_mm) {
            Some(format!("custom_{short_edge:.1}x{long_edge:.1}mm"))
        } else {
            return Err(AppError::invalid_input(
                "the raster page size is outside ShaPrint's supported media range",
            ));
        };
    }
    Ok(resolved)
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
        let resolved = settings_for_page(&PrintSettings::default(), &page).expect("resolves media");
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
        let resolved = settings_for_page(&PrintSettings::default(), &page).expect("resolves media");
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
        assert_eq!(
            settings_for_page(&requested, &page).expect("keeps explicit settings"),
            requested
        );
    }

    #[test]
    fn unknown_raster_size_is_not_relabelled_as_a4() {
        let page = RasterPage {
            width: 2400,
            height: 3900,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved = settings_for_page(&PrintSettings::default(), &page).expect("resolves media");
        assert_ne!(resolved.media.as_deref(), Some("iso_a4_210x297mm"));
        assert_eq!(resolved.media.as_deref(), Some("custom_203.2x330.2mm"));
    }

    #[test]
    fn raster_outside_the_legacy_custom_range_is_rejected() {
        let page = RasterPage {
            width: 4724,
            height: 5906,
            dpi: [300, 300],
            pixels: Vec::new(),
        };

        let error = settings_for_page(&PrintSettings::default(), &page).expect_err("rejects size");

        assert!(error
            .message()
            .contains("outside ShaPrint's supported media range"));
    }

    #[test]
    fn orientation_uses_physical_dimensions_when_dpi_differs_between_axes() {
        let page = RasterPage {
            width: 4961,
            height: 3508,
            dpi: [600, 300],
            pixels: Vec::new(),
        };
        let resolved = settings_for_page(&PrintSettings::default(), &page).expect("resolves media");
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
        let resolved_f4 =
            settings_for_page(&PrintSettings::default(), &f4_page).expect("resolves F4");
        assert_eq!(resolved_f4.media.as_deref(), Some("om_folio_210x330mm"));
        assert_eq!(resolved_f4.orientation, Some(PrintOrientation::Portrait));

        let folio_page = RasterPage {
            width: 3900,
            height: 2550,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved_folio =
            settings_for_page(&PrintSettings::default(), &folio_page).expect("resolves Folio");
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
    fn opaque_client_media_uses_f4_raster_geometry() {
        let page = RasterPage {
            width: 2480,
            height: 3898,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let requested = PrintSettings {
            media: Some("Unknown".to_owned()),
            ..PrintSettings::default()
        };

        let resolved = settings_for_page(&requested, &page).expect("resolves opaque media");

        assert_eq!(resolved.media.as_deref(), Some("om_folio_210x330mm"));
    }

    #[test]
    fn a6_and_photo_sizes_without_job_media_resolve_correctly() {
        let a6_page = RasterPage {
            width: 1240,
            height: 1748,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved_a6 =
            settings_for_page(&PrintSettings::default(), &a6_page).expect("resolves A6");
        assert_eq!(resolved_a6.media.as_deref(), Some("iso_a6_105x148mm"));

        let photo_page = RasterPage {
            width: 1200,
            height: 1800,
            dpi: [300, 300],
            pixels: Vec::new(),
        };
        let resolved_photo = settings_for_page(&PrintSettings::default(), &photo_page)
            .expect("resolves photo media");
        assert_eq!(resolved_photo.media.as_deref(), Some("oe_photo-4x6_4x6in"));
    }
}
