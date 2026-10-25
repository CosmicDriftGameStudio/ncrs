use std::path::PathBuf;

use ncrs_intake::{ImageTextSource, PageImage, TextError};
use objc2::rc::{autoreleasepool, Retained};
use objc2::AnyThread as _;
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use objc2_vision::{
    VNImageRequestHandler, VNObservation, VNRecognizeTextRequest, VNRecognizedTextObservation,
    VNRequest, VNRequestTextRecognitionLevel,
};

const PREFERRED_LANGUAGES: [&str; 2] = ["de-DE", "en-US"];

/// Reads page images with the text recognition that ships with macOS. It runs
/// on the calling thread and needs no models.
#[derive(Debug, Default, Clone, Copy)]
pub struct VisionText;

impl ImageTextSource for VisionText {
    fn recognise(&self, images: &[PageImage]) -> Result<Option<String>, TextError> {
        let mut pages = Vec::new();
        for page in images {
            let text = autoreleasepool(|_pool| read_page(page)).map_err(|reason| {
                TextError::Unreadable {
                    path: PathBuf::new(),
                    reason: format!("text recognition failed: {reason}"),
                }
            })?;
            if !text.trim().is_empty() {
                pages.push(text);
            }
        }
        Ok((!pages.is_empty()).then(|| pages.join("\n")))
    }
}

fn read_page(page: &PageImage) -> Result<String, String> {
    let png = encode_png(page)?;
    let data = NSData::with_bytes(&png);
    let handler = VNImageRequestHandler::initWithData_options(
        VNImageRequestHandler::alloc(),
        &data,
        &NSDictionary::new(),
    );
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    if let Some(languages) = supported_languages(&request) {
        request.setRecognitionLanguages(&languages);
    }
    let as_request: &VNRequest = &request;
    handler
        .performRequests_error(&NSArray::from_slice(&[as_request]))
        .map_err(|err| err.localizedDescription().to_string())?;
    let mut lines: Vec<(f64, f64, String)> = results(as_request)
        .iter()
        .filter_map(|observation| observation.downcast::<VNRecognizedTextObservation>().ok())
        .filter_map(|observation| {
            let best = observation.topCandidates(1).firstObject()?;
            let frame = bounding_box(&observation);
            Some((
                frame.origin.y + frame.size.height,
                frame.origin.x,
                best.string().to_string(),
            ))
        })
        .collect();
    // Vision's origin is the lower left corner: the highest top edge is first.
    lines.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));
    Ok(lines
        .into_iter()
        .map(|(_top, _left, line)| line)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn encode_png(page: &PageImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, page.width, page.height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(&page.pixels)
        .map_err(|err| err.to_string())?;
    writer.finish().map_err(|err| err.to_string())?;
    Ok(out)
}

/// The preferred languages the request supports, `None` when Vision cannot say
/// (then its default stays).
fn supported_languages(request: &VNRecognizeTextRequest) -> Option<Retained<NSArray<NSString>>> {
    let supported = list_supported_languages(request)?;
    let wanted: Vec<Retained<NSString>> = PREFERRED_LANGUAGES
        .iter()
        .map(|language| NSString::from_str(language))
        .filter(|language| supported.containsObject(language))
        .collect();
    (!wanted.is_empty()).then(|| NSArray::from_retained_slice(&wanted))
}

#[allow(unsafe_code)]
fn list_supported_languages(
    request: &VNRecognizeTextRequest,
) -> Option<Retained<NSArray<NSString>>> {
    // SAFETY: `request` is a live VNRecognizeTextRequest and the method takes no
    // other input; the unsafe marker comes from the generated binding. An
    // error is turned into None, never dereferenced.
    unsafe { request.supportedRecognitionLanguagesAndReturnError() }.ok()
}

#[allow(unsafe_code)]
fn results(request: &VNRequest) -> Retained<NSArray<VNObservation>> {
    // SAFETY: called after `performRequests:error:` returned; `results` is nil
    // when the request failed, which becomes an empty array. Every element is
    // type-checked by `downcast` before it is used as an observation subclass.
    unsafe { request.results() }.unwrap_or_default()
}

#[allow(unsafe_code)]
fn bounding_box(observation: &VNRecognizedTextObservation) -> CGRect {
    // SAFETY: `observation` is a live object that Vision returned; boundingBox
    // is a plain getter returning a CGRect by value.
    unsafe { observation.boundingBox() }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use ncrs_intake::test_support::{pdf_text, rendered_pages, text_pdf, write};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn the_words_of_a_rendered_page_come_back() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "invoice.pdf", &text_pdf("Invoice 4711 Company A"));
        let pages = rendered_pages(&pdf_text(), &path);
        let text = VisionText.recognise(&pages).unwrap().expect("text");
        let lower = text.to_lowercase();
        for word in ["invoice", "4711", "company"] {
            assert!(lower.contains(word), "{word} missing in {text:?}");
        }
    }

    #[test]
    fn a_blank_page_yields_no_text_and_no_error() {
        let blank = PageImage {
            width: 200,
            height: 200,
            pixels: vec![255; 200 * 200],
        };
        assert_eq!(VisionText.recognise(&[blank]), Ok(None));
    }

    #[test]
    fn a_page_with_inconsistent_pixels_is_an_error() {
        let broken = PageImage {
            width: 10,
            height: 10,
            pixels: vec![0; 7],
        };
        assert!(matches!(
            VisionText.recognise(&[broken]),
            Err(TextError::Unreadable { .. })
        ));
    }
}
