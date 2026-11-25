use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use ncrs_ai::{AiError, ExtractedField, ExtractionRequest, FailureScope, Provider, CONFIDENT};
use ncrs_index::{
    ContentHash, Document, DocumentFields, DocumentId, DocumentStatus, Field, FieldOrigin,
    FieldOrigins, IndexError, KindCatalog, KindLanguage,
};
use ncrs_intake::{LocationMode, AI_FIELD_ORDER};

use crate::step::{with_index_value, IndexAccess, Item, Step, StepOutcome};
use crate::steps::NO_INDEX;

/// Bump when the AI reads more or better: documents are asked again.
pub const AI_READ_VERSION: u32 = 2;

/// Asks the provider for the fields that are empty or hold a value the rules
/// only guessed (`FieldOrigin::is_heuristic`). What comes back is only ever
/// field values: a sure one replaces an empty or guessed value, an unsure one
/// is dropped. Every document is asked once per `AI_READ_VERSION`, whatever
/// the answer; the index remembers it. The step runs after everything else
/// and only enriches: `Continue` means a field changed, `Unchanged` that
/// nothing did.
#[derive(Debug)]
pub struct AiStep {
    index: Arc<dyn IndexAccess>,
    provider: Arc<dyn Provider>,
    /// The names of the person using the app and their companies; a sender
    /// that is one of them is the addressee the provider mistook for the sender.
    own_names: Vec<String>,
    kind_language: KindLanguage,
    /// Failed answers per document in this run of the app. After
    /// `DOCUMENT_ATTEMPTS` the document counts as asked, so one it cannot
    /// answer is not sent again on every scan.
    failed_attempts: Mutex<HashMap<DocumentId, u8>>,
}

/// How often a document that makes the model fail is tried.
const DOCUMENT_ATTEMPTS: u8 = 3;

/// What the step looked at before the provider call, which ran without the
/// index.
struct Subject {
    document: Document,
    kinds: Arc<KindCatalog>,
    origins: FieldOrigins,
}

impl AiStep {
    pub fn new(
        index: Arc<dyn IndexAccess>,
        provider: Arc<dyn Provider>,
        own_names: Vec<String>,
        kind_language: KindLanguage,
    ) -> Self {
        Self {
            index,
            provider,
            own_names,
            kind_language,
            failed_attempts: Mutex::default(),
        }
    }

    /// Counts a failure of the document itself; true once it has had its tries.
    fn out_of_attempts(&self, item: &Item) -> bool {
        let Some(id) = item.document else {
            return false;
        };
        let mut attempts = self
            .failed_attempts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = attempts.entry(id).or_insert(0);
        *count = count.saturating_add(1);
        *count >= DOCUMENT_ATTEMPTS
    }

    /// The document and the kinds the index knows, unless the document is
    /// gone or was filed or is being decided elsewhere.
    fn subject(&self, item: &Item) -> Result<Option<Subject>, String> {
        let id = document_id(item)?;
        let (document, kinds, origins) = with_index_value(self.index.as_ref(), |index| {
            Ok::<_, IndexError>((
                index.document(id)?,
                Arc::clone(index.kinds()),
                index.field_origins(id)?,
            ))
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map_err(|err| err.to_string())?;
        Ok(document
            .filter(|found| is_open_for_fields(item.location.mode, found.status))
            .map(|open_document| Subject {
                document: open_document,
                kinds,
                origins,
            }))
    }

    /// The text the earlier steps stored; an item that waited for this step
    /// arrives without it.
    fn text_of(&self, item: &Item) -> Result<Option<String>, String> {
        if let Some(text) = &item.text {
            return Ok(Some(text.clone()));
        }
        let id = document_id(item)?;
        let stored = with_index_value(self.index.as_ref(), |index| index.document_text(id))
            .ok_or_else(|| NO_INDEX.to_owned())?
            .map_err(|err| err.to_string())?;
        Ok(stored.filter(|text| !text.is_empty()))
    }

    fn asked_this_version(&self, item: &Item) -> Result<bool, String> {
        let id = document_id(item)?;
        with_index_value(self.index.as_ref(), |index| index.ai_read_version(id))
            .ok_or_else(|| NO_INDEX.to_owned())?
            .map(|version| version >= AI_READ_VERSION)
            .map_err(|err| err.to_string())
    }

    fn remember_asked(&self, item: &Item) -> Result<(), String> {
        let id = document_id(item)?;
        with_index_value(self.index.as_ref(), |index| {
            index.set_ai_read_version(id, AI_READ_VERSION)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map_err(|err| err.to_string())
    }
}

fn document_id(item: &Item) -> Result<DocumentId, String> {
    item.document
        .ok_or_else(|| "the file has no document".to_owned())
}

/// An inbox document that is filed or planned is nobody's business here.
fn is_open_for_fields(mode: LocationMode, status: DocumentStatus) -> bool {
    match mode {
        LocationMode::Inbox => matches!(status, DocumentStatus::New | DocumentStatus::InReview),
        LocationMode::Archive => true,
    }
}

const SALUTATIONS: [&str; 8] = ["herr", "frau", "hr", "fr", "mr", "mrs", "ms", "dr"];

/// Lowercase words without surrounding punctuation, leading salutations dropped.
fn name_words(text: &str) -> Vec<String> {
    let words = text
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let salutations = words
        .iter()
        .take_while(|word| SALUTATIONS.contains(&word.as_str()))
        .count();
    words.into_iter().skip(salutations).collect()
}

/// Whether the sender contains the user's own name as a whole sequence of
/// words, ignoring case, salutations and spacing. A one-word name is only
/// the user when it is the whole sender: it is also the start of companies
/// ("Marc GmbH"). Without an own name nothing is ever the user.
pub fn sender_is_the_user(sender: &str, own_name: Option<&str>) -> bool {
    let own = own_name.map(name_words).unwrap_or_default();
    if own.is_empty() {
        return false;
    }
    let sender_words = name_words(sender);
    if own.len() == 1 {
        return sender_words == own;
    }
    sender_words
        .windows(own.len())
        .any(|window| window == own.as_slice())
}

fn is_open(fields: &DocumentFields, field: Field) -> bool {
    match field {
        Field::Sender => fields.sender.is_none(),
        Field::Recipient => fields.recipient.is_none(),
        Field::Kind => fields.kind.is_none(),
        Field::Date => fields.date.is_none(),
        Field::Number => fields.number.is_none(),
        Field::Amount => fields.amount.is_none(),
    }
}

pub fn clear_field(fields: &mut DocumentFields, field: Field) {
    match field {
        Field::Sender => fields.sender = None,
        Field::Recipient => fields.recipient = None,
        Field::Kind => fields.kind = None,
        Field::Date => fields.date = None,
        Field::Number => fields.number = None,
        Field::Amount => fields.amount = None,
    }
}

impl Step for AiStep {
    fn name(&self) -> &'static str {
        "ai"
    }

    fn waits_for_the_rest(&self) -> bool {
        true
    }

    fn run(&self, item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        if !item.force && self.asked_this_version(item)? {
            return Ok(StepOutcome::Unchanged);
        }
        let Some(text) = self.text_of(item)? else {
            self.remember_asked(item)?;
            return Ok(StepOutcome::Unchanged);
        };
        let Some(Subject {
            document,
            kinds,
            origins,
        }) = self.subject(item)?
        else {
            return Ok(StepOutcome::Unchanged);
        };
        let open_fields: Vec<Field> = AI_FIELD_ORDER
            .into_iter()
            .filter(|&field| is_open(&document.fields, field) || origins.of(field).is_heuristic())
            .collect();
        // The recipient alone is no reason to send the text anywhere.
        if open_fields.iter().all(|&field| field == Field::Recipient) {
            self.remember_asked(item)?;
            return Ok(StepOutcome::Unchanged);
        }
        // A guessed value must not steer the provider towards the wrong one.
        let mut known = document.fields.clone();
        for &field in &open_fields {
            clear_field(&mut known, field);
        }
        let request = ExtractionRequest {
            text,
            open_fields: open_fields.clone(),
            known,
            source: item.location.path.clone(),
            kinds,
            language: self.kind_language,
            judge_document: false,
        };
        let extraction = match self.provider.extract(&request) {
            Ok(extraction) => extraction,
            // Nothing was sent; the user is asked elsewhere.
            Err(AiError::ConsentRequired(_)) => return Ok(StepOutcome::Unchanged),
            // Only a failure of the document itself counts against it; offline
            // or a refused key says nothing about the document.
            Err(error) => {
                if self.provider.failure_scope(&error) == FailureScope::Document
                    && self.out_of_attempts(item)
                {
                    self.remember_asked(item)?;
                }
                return Ok(StepOutcome::Unchanged);
            }
        };
        let confident: Vec<&ExtractedField> = extraction
            .fields
            .iter()
            .filter(|found| open_fields.contains(&found.field))
            .filter(|found| {
                found.field != Field::Sender
                    || !self
                        .own_names
                        .iter()
                        .any(|own| sender_is_the_user(&found.value, Some(own)))
            })
            .filter(|found| found.confidence >= CONFIDENT)
            .collect();
        let changed = self.write_fields(item, &confident, &document.hash)?;
        // Asked and answered, also when nothing was found.
        self.remember_asked(item)?;
        Ok(if changed {
            StepOutcome::Continue
        } else {
            StepOutcome::Unchanged
        })
    }
}

/// `confidence` is 0.0 to 1.0; counting the percents below it avoids a lossy
/// float-to-integer cast.
fn confidence_percent(confidence: f32) -> u8 {
    let scaled = confidence * 100.0;
    let rounded_percent = (1..=100)
        .filter(|percent: &u8| f32::from(*percent) - 0.5 <= scaled)
        .count();
    u8::try_from(rounded_percent).unwrap_or(100)
}

impl AiStep {
    /// Writes the confident fields over the empty and the guessed ones, reading
    /// the document again first: the provider call ran without the index. A
    /// document that is gone, has other bytes or is no longer open for fields
    /// is left alone. True when a field changed.
    fn write_fields(
        &self,
        item: &Item,
        confident: &[&ExtractedField],
        asked_about: &ContentHash,
    ) -> Result<bool, String> {
        let id = document_id(item)?;
        with_index_value(self.index.as_ref(), |index| {
            let Some(current) = index.document(id)? else {
                return Ok(false);
            };
            if current.hash != *asked_about
                || !is_open_for_fields(item.location.mode, current.status)
            {
                return Ok(false);
            }
            let origins = index.field_origins(id)?;
            let mut merged = current.fields;
            let mut changed = false;
            for found in confident {
                changed |= found.write(
                    &mut merged,
                    index.kinds(),
                    origins.of(found.field).is_heuristic(),
                );
            }
            if changed {
                index.set_document_fields(id, &merged, |field| {
                    confident.iter().find(|found| found.field == field).map_or(
                        FieldOrigin::Unknown,
                        |found| FieldOrigin::Ai {
                            confidence_percent: confidence_percent(found.confidence),
                        },
                    )
                })?;
            }
            Ok::<_, IndexError>(changed)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests;
