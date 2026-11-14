//! Pairing QR codes.
//!
//! The code encodes the *address* of the host plus the short-lived pairing
//! code — never a device token and never a permanent credential. The token a
//! device ends up holding is minted by the host only after the operator
//! approves the request, so a photograph of the screen that is taken after the
//! code expires reveals nothing that can be replayed.

use qrcode::render::svg;
use qrcode::QrCode;

use super::model::PAIRING_TTL_MS;

/// A QR code plus the URL it encodes, as handed to the Settings page.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCode {
    /// The short-lived code the operator reads out, if the camera is unavailable.
    pub code: String,
    /// URL the QR code points at, e.g. `http://192.168.1.20:8722/#pair=123456`.
    pub url: String,
    /// SVG markup of the QR code, ready to be embedded in the page.
    pub svg: String,
    /// Milliseconds from now until the code stops working.
    pub expires_in_ms: i64,
}

/// Renders `url` as an SVG QR code.
///
/// Rendered dark-on-light on purpose: a scanner expects a light background, and
/// an inverted code is frequently rejected by phone cameras.
pub fn render_svg(url: &str) -> Result<String, String> {
    let code = QrCode::new(url.as_bytes())
        .map_err(|error| format!("Could not encode the pairing code: {error}"))?;

    Ok(code
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .dark_color(svg::Color("#09090B"))
        .light_color(svg::Color("#FFFFFF"))
        .build())
}

/// Builds the URL a remote device should open.
///
/// The pairing code travels in the fragment, which a browser never sends to the
/// server: it stays on the device and cannot leak into a server log.
pub fn pairing_url(base: &str, code: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    format!("{trimmed}/#pair={code}")
}

/// The lifetime of a freshly issued code, re-exported so the IPC layer does not
/// have to reach into the model.
pub fn pairing_ttl_ms() -> i64 {
    PAIRING_TTL_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_renders_to_an_svg_document() {
        let svg = render_svg("http://192.168.1.20:8722/#pair=123456").expect("the URL fits a QR code");
        assert!(svg.contains("<svg"), "an SVG document is returned: {svg:.60}");
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("path") || svg.contains("rect"));
    }

    #[test]
    fn the_pairing_url_keeps_the_code_out_of_the_query_string() {
        let url = pairing_url("http://192.168.1.20:8722/", "123456");
        assert_eq!(url, "http://192.168.1.20:8722/#pair=123456");
        assert!(!url.contains("?"));
    }

    #[test]
    fn a_trailing_slash_is_not_doubled() {
        assert_eq!(
            pairing_url("http://10.0.0.5:8722", "000001"),
            "http://10.0.0.5:8722/#pair=000001"
        );
    }
}
