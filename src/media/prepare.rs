//! Turn an X photo into the two forms the app needs: a decoded image for the
//! terminal, and a JPEG that fits Jev's image limits.

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageReader, Rgb, RgbImage};

use crate::api::types::{Media, Tweet};

/// Jev rejects an image with a side under this many pixels
/// (docs.typesafe.ai/images#limits).
pub const JEV_MIN_SIDE: u32 = 280;
/// Jev rejects an image with a side over this many pixels.
pub const JEV_MAX_SIDE: u32 = 1568;
/// JPEG quality of the copy sent to Jev and to embedding models.
const JPEG_QUALITY: u8 = 85;

/// A post whose text, without links, mentions and hashtags, is shorter than
/// this many characters counts as "thin": its photo carries the content.
pub const THIN_TEXT_CHARS: usize = 60;

/// URL of the `name=small` JPEG variant of an X photo (680 px on the long
/// side): large enough for the terminal and inside Jev's size range.
///
/// `https://pbs.twimg.com/media/ABC.png` becomes
/// `https://pbs.twimg.com/media/ABC?format=jpg&name=small`. A URL without
/// an extension, or one that already has a query, is returned as it is.
pub fn small_variant_url(url: &str) -> String {
    if url.contains('?') {
        return url.to_string();
    }
    let Some((stem, ext)) = url.rsplit_once('.') else {
        return url.to_string();
    };
    if ext.contains('/') {
        return url.to_string();
    }
    format!("{stem}?format=jpg&name=small")
}

/// The first photo of a tweet, looked up by media key. Videos and GIFs are
/// skipped: their preview frame is not the content.
pub fn first_photo<'a>(
    tweet: &Tweet,
    lookup: impl Fn(&str) -> Option<&'a Media>,
) -> Option<&'a Media> {
    photos(tweet, lookup).next()
}

/// Every photo of a tweet, in attachment order.
pub fn photos<'a, 't>(
    tweet: &'t Tweet,
    lookup: impl Fn(&str) -> Option<&'a Media> + 't,
) -> impl Iterator<Item = &'a Media> + 't {
    tweet
        .attachments
        .as_ref()
        .and_then(|a| a.media_keys.as_ref())
        .into_iter()
        .flatten()
        .filter_map(move |key| lookup(key))
        .filter(|m| m.type_ == "photo" && m.url.is_some())
}

/// The text a reader sees as content: links, @mentions and #hashtags removed.
pub fn bare_text(text: &str) -> String {
    text.split_whitespace()
        .filter(|w| !(w.starts_with("http://") || w.starts_with("https://")))
        .filter(|w| !(w.starts_with('@') || w.starts_with('#')))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a post's photo carries more of its content than its text does.
pub fn is_thin_text(text: &str) -> bool {
    bare_text(text).chars().count() < THIN_TEXT_CHARS
}

/// Decode an image and rotate its pixels upright from the EXIF orientation.
/// Jev and the embedding models ignore EXIF, so the pixels must be upright.
pub fn decode_upright(bytes: &[u8]) -> Result<DynamicImage, image::ImageError> {
    let mut decoder = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// Fit an image to Jev's limits: shrink the long side to `JEV_MAX_SIDE`,
/// then pad a short side under `JEV_MIN_SIDE` with black. Never upscales,
/// so the model sees the photo's own detail.
pub fn fit_for_jev(image: &DynamicImage) -> DynamicImage {
    let image = if image.width().max(image.height()) > JEV_MAX_SIDE {
        image.thumbnail(JEV_MAX_SIDE, JEV_MAX_SIDE)
    } else {
        image.clone()
    };
    let (w, h) = image.dimensions();
    if w.min(h) >= JEV_MIN_SIDE {
        return image;
    }
    let (cw, ch) = (w.max(JEV_MIN_SIDE), h.max(JEV_MIN_SIDE));
    let mut canvas = RgbImage::from_pixel(cw, ch, Rgb([0, 0, 0]));
    image::imageops::overlay(
        &mut canvas,
        &image.to_rgb8(),
        i64::from((cw - w) / 2),
        i64::from((ch - h) / 2),
    );
    DynamicImage::ImageRgb8(canvas)
}

/// Encode as a baseline JPEG and return it as a `data:` URL, the form both
/// the OpenRouter Decisions API and the embedding endpoints accept.
pub fn jpeg_data_url(image: &DynamicImage) -> Result<String, image::ImageError> {
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY).encode_image(&image.to_rgb8())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&jpeg);
    Ok(format!("data:image/jpeg;base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_variant_replaces_the_extension_with_a_query() {
        assert_eq!(
            small_variant_url("https://pbs.twimg.com/media/HU502HFX0AAMWSH.jpg"),
            "https://pbs.twimg.com/media/HU502HFX0AAMWSH?format=jpg&name=small"
        );
        assert_eq!(
            small_variant_url("https://pbs.twimg.com/media/X.png"),
            "https://pbs.twimg.com/media/X?format=jpg&name=small"
        );
    }

    #[test]
    fn small_variant_keeps_urls_it_cannot_rewrite() {
        let with_query = "https://pbs.twimg.com/media/X?format=jpg&name=large";
        assert_eq!(small_variant_url(with_query), with_query);
        let no_ext = "https://example.com/media/photo";
        assert_eq!(small_variant_url(no_ext), no_ext);
    }

    #[test]
    fn bare_text_drops_links_mentions_and_tags() {
        assert_eq!(
            bare_text("@alice にんにくラーメン🧄 #ramen https://t.co/x"),
            "にんにくラーメン🧄"
        );
    }

    #[test]
    fn thin_text_counts_characters_not_bytes() {
        // 30 CJK characters are 90 bytes but still thin.
        assert!(is_thin_text(&"麺".repeat(30)));
        assert!(!is_thin_text(&"a".repeat(THIN_TEXT_CHARS)));
        assert!(is_thin_text(&format!(
            "{} https://t.co/abc",
            "a".repeat(59)
        )));
    }

    #[test]
    fn large_images_shrink_to_the_jev_maximum() {
        let img = DynamicImage::new_rgb8(4000, 3000);
        let fitted = fit_for_jev(&img);
        assert_eq!(fitted.width(), JEV_MAX_SIDE);
        assert!(fitted.height() <= JEV_MAX_SIDE);
    }

    #[test]
    fn small_sides_are_padded_not_upscaled() {
        let img = DynamicImage::new_rgb8(680, 200);
        let fitted = fit_for_jev(&img);
        assert_eq!(fitted.dimensions(), (680, JEV_MIN_SIDE));
        let tiny = DynamicImage::new_rgb8(100, 50);
        assert_eq!(
            fit_for_jev(&tiny).dimensions(),
            (JEV_MIN_SIDE, JEV_MIN_SIDE)
        );
    }

    #[test]
    fn images_inside_the_limits_are_unchanged() {
        let img = DynamicImage::new_rgb8(680, 383);
        assert_eq!(fit_for_jev(&img).dimensions(), (680, 383));
    }

    #[test]
    fn data_url_holds_a_decodable_jpeg() {
        let url = jpeg_data_url(&DynamicImage::new_rgb8(300, 300)).unwrap();
        let b64 = url.strip_prefix("data:image/jpeg;base64,").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        assert_eq!(decode_upright(&bytes).unwrap().dimensions(), (300, 300));
    }
}
