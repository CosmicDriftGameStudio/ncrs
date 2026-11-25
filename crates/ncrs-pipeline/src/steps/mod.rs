use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use ncrs_ai::Provider;
use ncrs_index::{
    ContentHash, DocumentId, DocumentStatus, Field, FieldOrigin, Index, IndexError, KindLanguage,
};
use ncrs_intake::{
    detect_fields, OwnNames, StabilityTracker, TextChain, TextError, FIELDS_READ_VERSION,
};
use ncrs_intake::{
    finish_take_in, prepare_take_in, settle_gone_path, settle_location, IntakeError, Location,
    LocationMode, Outcome, PendingHash, Prepared,
};
use ncrs_intake::{open_below, text_is_readable, ChainReading};

use crate::ai_step::{clear_field, AiStep, AI_READ_VERSION};
use crate::step::{with_index_value, IndexAccess, Item, Step, StepOutcome};

pub const NO_INDEX: &str = "index not open";

/// What belongs to the user and never to a sender.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnDetails {
    /// The user's full name and the names of their companies.
    pub names: Vec<String>,
    pub ibans: Vec<String>,
    pub tax_ids: Vec<String>,
    /// The language a `{kind}` folder is written in.
    pub kind_language: KindLanguage,
}

/// The steps every file passes, in order: intake, text, fields. The AI step
/// comes last and only when a provider is given; it waits for all other work.
/// No step decides where a document goes.
pub fn intake_steps(
    index: Arc<dyn IndexAccess>,
    text: Arc<TextChain>,
    ai: Option<Arc<dyn Provider>>,
    own: OwnDetails,
) -> Vec<Box<dyn Step>> {
    let mut steps: Vec<Box<dyn Step>> = vec![
        Box::new(IntakeStep {
            index: Arc::clone(&index),
            tracker: Mutex::new(StabilityTracker::new()),
            ai: ai.clone(),
        }),
        Box::new(TextStep {
            index: Arc::clone(&index),
            chain: text,
        }),
        Box::new(FieldsStep {
            index: Arc::clone(&index),
            own: own.clone(),
        }),
    ];
    if let Some(provider) = ai {
        steps.push(Box::new(AiStep::new(
            index,
            provider,
            own.names,
            own.kind_language,
        )));
    }
    steps
}
enum Ready {
    Outcome(Outcome),
    Hashed(PendingHash, ContentHash),
}

/// Takes the file into the index, once it has held still.
#[derive(Debug)]
struct IntakeStep {
    index: Arc<dyn IndexAccess>,
    /// A document the AI was never asked goes on again, so it is asked once,
    /// but only while the provider can answer (checked on every file, so a
    /// model loaded later counts without a restart).
    ai: Option<Arc<dyn Provider>>,
    tracker: Mutex<StabilityTracker>,
}

impl IntakeStep {
    fn can_ask_ai(&self, source: &Path) -> bool {
        self.ai.as_ref().is_some_and(|ai| ai.can_answer(source))
    }

    fn ai_is_due(
        &self,
        index: &Index,
        id: DocumentId,
        location: &Location,
    ) -> Result<bool, IndexError> {
        Ok(self.can_ask_ai(&location.path) && index.ai_read_version(id)? < AI_READ_VERSION)
    }

    /// A new inbox document goes on while something is left to read: its
    /// text or fields were never stored, or the AI was never asked. One in
    /// review goes on only to be read; its fields are left alone.
    fn inbox_document_continues(
        &self,
        index: &mut Index,
        id: DocumentId,
        item: &mut Item,
    ) -> Result<bool, IndexError> {
        let Some(document) = index.document(id)? else {
            return Ok(false);
        };
        match document.status {
            // A file whose text was stored empty could not be read: the same
            // bytes are not parsed again on every scan. Reading it again is
            // asked for by force.
            DocumentStatus::New => {
                let stored = index.document_text(id)?;
                let stored_empty = stored
                    .as_deref()
                    .is_some_and(|text| text.trim().is_empty());
                if stored_empty {
                    return Ok(false);
                }
                let continues = stored.is_none()
                    || index.fields_read_version(id)? < FIELDS_READ_VERSION
                    || self.ai_is_due(index, id, &item.location)?;
                // Text stored for these bytes is not read (PDF, OCR) again.
                item.stored_text_is_current = continues && stored.is_some();
                Ok(continues)
            }
            DocumentStatus::InReview => {
                let has_text = index.has_document_text_row(id)?;
                let continues = text_is_readable(&item.path)
                    && (!has_text || self.ai_is_due(index, id, &item.location)?);
                item.stored_text_is_current = continues && has_text;
                Ok(continues)
            }
            DocumentStatus::Planned | DocumentStatus::Filed => Ok(false),
        }
    }
}

impl Step for IntakeStep {
    fn name(&self) -> &'static str {
        "intake"
    }

    fn run(&self, item: &mut Item, now: SystemTime) -> Result<StepOutcome, String> {
        // The hash reads the whole file, so the index is let go of meanwhile:
        // every worker shares it, and the UI reaches it too.
        let prepared = with_index_value(self.index.as_ref(), |index| {
            let mut tracker = self.tracker.lock().unwrap_or_else(PoisonError::into_inner);
            prepare_take_in(index, &item.location, &item.path, &mut tracker, now)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?;
        let ready = prepared.and_then(|step| match step {
            Prepared::Done(taken) => Ok(Ready::Outcome(taken.outcome)),
            Prepared::Hash(pending) => Ok(match pending.hash(&item.path)? {
                Some(hash) => Ready::Hashed(pending, hash),
                None => Ready::Outcome(Outcome::NotStableYet),
            }),
        });
        let taken = with_index_value(self.index.as_ref(), |index| {
            let outcome = match ready? {
                Ready::Outcome(outcome) => outcome,
                Ready::Hashed(pending, hash) => {
                    finish_take_in(index, &item.location, &item.path, pending, hash, now)?.outcome
                }
            };
            // An inbox document taken in but never read (the app quit in
            // between) goes on again, and so does an archive document whose
            // text was never read, for example for lack of the PDF library.
            let unreviewed = match outcome {
                Outcome::Unchanged(_) if item.force => true,
                Outcome::Unchanged(id) => match item.location.mode {
                    LocationMode::Inbox => self.inbox_document_continues(index, id, item)?,
                    LocationMode::Archive => {
                        let has_text = index.has_document_text_row(id)?;
                        let continues = text_is_readable(&item.path)
                            && (!has_text
                                || index.fields_read_version(id)? < FIELDS_READ_VERSION
                                || self.ai_is_due(index, id, &item.location)?);
                        item.stored_text_is_current = continues && has_text;
                        continues
                    }
                },
                _ => false,
            };
            Ok::<_, IntakeError>((outcome, unreviewed))
        })
        .ok_or_else(|| NO_INDEX.to_owned())?;
        match taken {
            Ok((outcome, unreviewed)) => Ok(match outcome {
                Outcome::NotStableYet => StepOutcome::NotReady,
                Outcome::New(id) | Outcome::Moved(id) | Outcome::ContentChanged(id) => {
                    item.document = Some(id);
                    StepOutcome::Continue
                }
                Outcome::Unchanged(id) if unreviewed => {
                    item.document = Some(id);
                    StepOutcome::Continue
                }
                Outcome::Unchanged(_)
                | Outcome::Copy(_)
                | Outcome::AlreadyFiled(_)
                | Outcome::Skipped(_) => StepOutcome::Unchanged,
            }),
            // The next scan settles a file that vanished.
            Err(IntakeError::Io {
                source,
                path: _path,
            }) if source.kind() == io::ErrorKind::NotFound => Ok(StepOutcome::Unchanged),
            Err(err) => Err(err.to_string()),
        }
    }

    fn after_scan(&self, location: &Location) -> Result<Vec<PathBuf>, String> {
        with_index_value(self.index.as_ref(), |index| {
            settle_location(index, location)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map(|report| report.error_paths)
        .map_err(|err| err.to_string())
    }

    fn path_gone(&self, location: &Location, path: &Path) -> Result<Vec<PathBuf>, String> {
        with_index_value(self.index.as_ref(), |index| {
            settle_gone_path(index, location, path)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map(|report| report.error_paths)
        .map_err(|err| err.to_string())
    }
}

/// Reads the text of the file and stores it in the index. A file without text
/// goes on with an empty text stored, so a later scan can tell "read, nothing
/// found" from "never read". Without the PDF library the file is held back
/// unchanged. The text of a document is only ever stored, never acted on.
#[derive(Debug)]
struct TextStep {
    index: Arc<dyn IndexAccess>,
    chain: Arc<TextChain>,
}

impl Step for TextStep {
    fn name(&self) -> &'static str {
        "text"
    }

    fn run(&self, item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        let id = item
            .document
            .ok_or_else(|| "the file has no document".to_owned())?;
        if item.force {
            self.chain.forget_crash(&item.path);
        }
        if item.stored_text_is_current {
            item.text = with_index_value(self.index.as_ref(), |index| index.document_text(id))
                .ok_or_else(|| NO_INDEX.to_owned())?
                .map_err(|err| err.to_string())?
                .filter(|text| !text.is_empty());
            return Ok(StepOutcome::Continue);
        }
        let reading = open_below(&item.location.path, &item.path)
            .map_err(|err| TextError::Io {
                path: item.path.clone(),
                reason: err.to_string(),
            })
            .and_then(|file| self.chain.read_open(&item.path, &file));
        item.text = match reading {
            Ok(ChainReading::Text(text)) => Some(text),
            Ok(ChainReading::NoText { .. }) => None,
            Err(TextError::LibraryMissing { .. }) => return Ok(StepOutcome::Unchanged),
            Err(err) => {
                // Without a stored text the document would be read (and fail)
                // again on every scan. Only the file itself being unreadable
                // is permanent: once the models work, it is read.
                if matches!(
                    err,
                    TextError::Unreadable { .. } | TextError::Quarantined { .. }
                ) {
                    let stored = with_index_value(self.index.as_ref(), |index| {
                        index.set_document_text(id, "")
                    });
                    if let Some(Err(store_err)) = stored {
                        return Err(format!("{err}; {store_err}"));
                    }
                }
                return Err(err.to_string());
            }
        };
        let stored = item.text.as_deref().unwrap_or_default();
        with_index_value(self.index.as_ref(), |index| {
            index.set_document_text(id, stored)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map_err(|err| err.to_string())?;
        Ok(StepOutcome::Continue)
    }
}

/// The fields the detection of the look of a letter reads.
const DETECTED_FIELDS: [Field; 4] = [Field::Sender, Field::Recipient, Field::Kind, Field::Date];

/// Fills the empty fields of a document from its text, without a model, and
/// reads again the ones that hold only a guess of an earlier detection. It
/// notes which version of the detection read the document. Nothing is planned
/// or filed, no review is opened and the status never changes; what happens
/// to the document is decided later. Fields a person set stay.
#[derive(Debug)]
struct FieldsStep {
    index: Arc<dyn IndexAccess>,
    own: OwnDetails,
}

impl FieldsStep {
    fn own_names(&self) -> OwnNames<'_> {
        OwnNames {
            names: &self.own.names,
            ibans: &self.own.ibans,
            tax_ids: &self.own.tax_ids,
        }
    }

    /// An inbox document that someone looks at or has filed keeps its fields.
    fn read_fields(
        &self,
        id: DocumentId,
        text: &str,
        only_new: bool,
    ) -> Result<StepOutcome, String> {
        with_index_value(self.index.as_ref(), |index| {
            let Some(document) = index.document(id)? else {
                return Ok(());
            };
            if only_new && document.status != DocumentStatus::New {
                return Ok(());
            }
            let origins = index.field_origins(id)?;
            let mut current = document.fields.clone();
            for field in DETECTED_FIELDS {
                if origins.of(field).is_heuristic() {
                    clear_field(&mut current, field);
                }
            }
            let detected = detect_fields(
                text,
                &current,
                &index.confirmed_sender_aliases()?,
                &self.own_names(),
                index.kinds(),
            );
            let filled = detected.fill(&current);
            if filled != document.fields {
                index.set_document_fields(id, &filled, |field| match field {
                    Field::Recipient => FieldOrigin::AddressBlock,
                    Field::Kind => FieldOrigin::Heading,
                    _ => FieldOrigin::Detected,
                })?;
            }
            index.set_fields_read_version(id, FIELDS_READ_VERSION)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map_err(|err| err.to_string())?;
        Ok(StepOutcome::Continue)
    }
}

impl Step for FieldsStep {
    fn name(&self) -> &'static str {
        "fields"
    }

    fn run(&self, item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        let id = item
            .document
            .ok_or_else(|| "the file has no document".to_owned())?;
        let text = item.text.as_deref().unwrap_or_default();
        match item.location.mode {
            LocationMode::Archive => self.read_fields(id, text, false),
            // Nothing to detect and nothing for the model to read.
            LocationMode::Inbox if text.trim().is_empty() => Ok(StepOutcome::Finished),
            LocationMode::Inbox => self.read_fields(id, text, true),
        }
    }
}

#[cfg(test)]
mod intake_step_tests;
