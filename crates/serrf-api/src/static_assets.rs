use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "static-dist/"]
struct Assets;

pub fn lookup(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    lookup_in::<Assets>(path)
}

fn lookup_in<E: RustEmbed>(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    let resolved = if path.is_empty() { "index.html" } else { path };
    let file = E::get(resolved)?;
    Some((file.data, mime_for(resolved)))
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("json") | Some("map") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{lookup_in, mime_for};
    use rust_embed::RustEmbed;

    #[derive(RustEmbed)]
    #[folder = "tests/fixtures/static/"]
    struct TestAssets;

    #[test]
    fn empty_path_resolves_to_index_html() {
        let (bytes, mime) = lookup_in::<TestAssets>("").unwrap();
        assert_eq!(mime, "text/html");
        assert!(String::from_utf8(bytes.into_owned()).unwrap().contains("Hello SERRF fixture"));
    }

    #[test]
    fn index_html_path_resolves_directly() {
        let (_, mime) = lookup_in::<TestAssets>("index.html").unwrap();
        assert_eq!(mime, "text/html");
    }

    #[test]
    fn nested_asset_resolves_with_javascript_mime() {
        let (bytes, mime) = lookup_in::<TestAssets>("_next/static/x.js").unwrap();
        assert_eq!(mime, "text/javascript");
        assert!(String::from_utf8(bytes.into_owned()).unwrap().contains("fixture asset"));
    }

    #[test]
    fn missing_asset_returns_none() {
        assert!(lookup_in::<TestAssets>("missing.js").is_none());
    }

    #[test]
    fn unknown_extension_falls_back_to_octet_stream() {
        assert_eq!(mime_for("thing.unknownext"), "application/octet-stream");
    }
}
