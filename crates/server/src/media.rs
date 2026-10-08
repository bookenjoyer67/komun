//! The one way an uploaded image is decoded. Decoding happens off the async runtime under fixed
//! limits, and the claimed content type selects the only decoder allowed.

use std::io::Cursor;

use image::{imageops::FilterType, DynamicImage, ImageError, ImageFormat, ImageReader, Limits};

pub(crate) const MAX_IMAGE_SIDE_PX: u32 = 6000;
const MAX_DECODE_ALLOC_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rejected {
    UnsupportedType,
    NotTheClaimedType,
    TooLarge,
    Undecodable,
}

impl Rejected {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Rejected::UnsupportedType => "only PNG, JPEG and WebP images are accepted",
            Rejected::NotTheClaimedType => "the file is not the image type it was sent as",
            Rejected::TooLarge => "the image is over 6000 pixels on a side or too large to decode",
            Rejected::Undecodable => "invalid image",
        }
    }
}

pub(crate) fn claimed_format(content_type: &str) -> Option<ImageFormat> {
    match content_type {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

/// Decodes `bytes` as `claimed_type` and scales the result down to fit a `fit_within` square.
pub(crate) async fn decode_bounded(
    bytes: Vec<u8>,
    claimed_type: &str,
    fit_within: u32,
) -> Result<DynamicImage, Rejected> {
    let format = claimed_format(claimed_type).ok_or(Rejected::UnsupportedType)?;
    tokio::task::spawn_blocking(move || decode_and_fit(&bytes, format, fit_within))
        .await
        // A decoder that panics was fed this input, so the input is refused like any other bad file.
        .unwrap_or(Err(Rejected::Undecodable))
}

fn decode_and_fit(
    bytes: &[u8],
    format: ImageFormat,
    fit_within: u32,
) -> Result<DynamicImage, Rejected> {
    if image::guess_format(bytes).ok() != Some(format) {
        return Err(Rejected::NotTheClaimedType);
    }

    // The header is read before any pixel buffer exists, so an oversized image costs nothing.
    let (width, height) = bounded_reader(bytes, format)
        .into_dimensions()
        .map_err(rejected)?;
    if width > MAX_IMAGE_SIDE_PX || height > MAX_IMAGE_SIDE_PX {
        return Err(Rejected::TooLarge);
    }

    let decoded = bounded_reader(bytes, format).decode().map_err(rejected)?;
    if decoded.width() > fit_within || decoded.height() > fit_within {
        Ok(decoded.resize(fit_within, fit_within, FilterType::Lanczos3))
    } else {
        Ok(decoded)
    }
}

fn bounded_reader(bytes: &[u8], format: ImageFormat) -> ImageReader<Cursor<&[u8]>> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE_PX);
    limits.max_image_height = Some(MAX_IMAGE_SIDE_PX);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    reader
}

fn rejected(error: ImageError) -> Rejected {
    match error {
        ImageError::Limits(_) => Rejected::TooLarge,
        _ => Rejected::Undecodable,
    }
}
