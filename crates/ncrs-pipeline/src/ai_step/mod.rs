use std::sync::Arc;
use std::time::SystemTime;

use ncrs_ai::{AiError, ExtractedField, ExtractionRequest, Provider, CONFIDENT};
use ncrs_index::{
    Document, DocumentFields, DocumentStatus, Field, FieldOrigin, FieldOrigins, IndexError,
    KindCatalog, KindLanguage, Kinds, ReviewReason,
};
use ncrs_intake::{
    plan, relative_to_location, LocationMode, PlanInput, Planning, AI_FIELD_ORDER, FIELD_ORDER,
};

use crate::step::{with_index_value, IndexAccess, Item, Step, StepOutcome};
use crate::steps::NO_INDEX;

/// Bump when the AI reads more or better: archive documents are asked again.
pub const AI_READ_VERSION: u32 = 2;

/// How sure the provider must be that a file is no document for Hermes to
/// exclude it.
pub const NOT_A_DOCUMENT_CONFIDENCE: f32 = 0.9;

/// Asks the provider for the fields that are empty or hold a value the rules
/// only guessed (`FieldOrigin::is_heuristic`): for documents the rules could
/// not place, and for documents nobody plans. What comes back is only ever
/// field values: a sure one replaces an empty or guessed value, an unsure one
/// for an empty field is sent to the review list. A document nobody plans is
/// asked once per `AI_READ_VERSION`; the index remembers it.
#[derive(Debug)]
pub struct AiStep {
    index: Arc<dyn IndexAccess>,
    provider: Arc<dyn Provider>,
    /// The names of the person using the app and their companies; a sender
    /// that is one of them is the addressee the provider mistook for the sender.
    own_names: Vec<String>,
    kind_language: KindLanguage,
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
        }
    }

    /// The document and the kinds the index knows; a document being planned
    /// must still be new.
    fn document(
        &self,
        item: &Item,
        must_be_new: bool,
    ) -> Result<Option<(Document, Arc<KindCatalog>, FieldOrigins)>, String> {
        let id = item
            .document
            .ok_or_else(|| "the file has no document".to_owned())?;
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
            .filter(|found| !must_be_new || found.status == DocumentStatus::New)
            .map(|found| (found, kinds, origins)))
    }

    fn remember_asked(&self, item: &Item) -> Result<(), String> {
        let id = item
            .document
            .ok_or_else(|| "the file has no document".to_owned())?;
        with_index_value(self.index.as_ref(), |index| {
            index.set_ai_read_version(id, AI_READ_VERSION)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map_err(|err| err.to_string())
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

/// An unconfirmed rule needs the user whatever the provider says.
fn wants_fields(reason: ReviewReason) -> bool {
    matches!(
        reason,
        ReviewReason::NoMatchingRule | ReviewReason::MissingField(_)
    )
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

    fn run(&self, item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        let review_reason = item.review.filter(|&reason| wants_fields(reason));
        let fields_only = item.review.is_none() && item.plan.is_none();
        if review_reason.is_none() && !fields_only {
            return Ok(StepOutcome::Continue);
        }
        if item.not_a_document {
            if fields_only {
                self.remember_asked(item)?;
            }
            return Ok(StepOutcome::Continue);
        }
        let Some(text) = item.text.as_deref() else {
            if fields_only {
                self.remember_asked(item)?;
            }
            return Ok(StepOutcome::Continue);
        };
        let Some((document, kinds, origins)) = self.document(item, review_reason.is_some())? else {
            return Ok(StepOutcome::Continue);
        };
        let open_fields: Vec<Field> = AI_FIELD_ORDER
            .into_iter()
            .filter(|&field| is_open(&document.fields, field) || origins.of(field).is_heuristic())
            .collect();
        // The recipient alone is no reason to send the text anywhere.
        if open_fields.iter().all(|&field| field == Field::Recipient) {
            if fields_only {
                self.remember_asked(item)?;
            }
            return Ok(StepOutcome::Continue);
        }
        // A guessed value must not steer the provider towards the wrong one.
        let mut known = document.fields.clone();
        for &field in &open_fields {
            clear_field(&mut known, field);
        }
        let request = ExtractionRequest {
            text: text.to_owned(),
            open_fields: open_fields.clone(),
            known,
            source: item.location.path.clone(),
            kinds,
            judge_document: item.location.mode == LocationMode::Inbox,
        };
        let extraction = match self.provider.extract(&request) {
            Ok(extraction) => extraction,
            // Nothing was sent; the user is asked elsewhere, and the document
            // goes on to the review list as it was.
            Err(AiError::ConsentRequired(_)) => return Ok(StepOutcome::Continue),
            // A provider failure (offline, timeout, broken reply) is not the
            // document's failure: an `Err` would drop the item from the review
            // list, so it continues with its reason and fields as they were.
            Err(_) => return Ok(StepOutcome::Continue),
        };
        // Nothing it read is worth keeping from a file that is no document,
        // and a rule must not plan it.
        if extraction
            .not_a_document
            .is_some_and(|sure| sure >= NOT_A_DOCUMENT_CONFIDENCE)
        {
            item.not_a_document = true;
            if fields_only {
                self.remember_asked(item)?;
            }
            return Ok(StepOutcome::Continue);
        }
        let (confident, mut unsure): (Vec<&ExtractedField>, Vec<&ExtractedField>) = extraction
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
            .partition(|found| found.confidence >= CONFIDENT);
        // An unsure value never replaces a guess, so it is no reason to ask a person.
        unsure.retain(|found| is_open(&document.fields, found.field));
        let least_sure = FIELD_ORDER
            .into_iter()
            .find(|&field| unsure.iter().any(|found| found.field == field));
        let plan_again =
            matches!(review_reason, Some(ReviewReason::MissingField(_))) && least_sure.is_none();
        let planning =
            self.write_and_plan(item, &confident, plan_again, review_reason.is_some())?;
        if fields_only {
            self.remember_asked(item)?;
            return Ok(StepOutcome::Continue);
        }

        if let Some(field) = least_sure {
            item.review = Some(ReviewReason::LowConfidence(field));
            return Ok(StepOutcome::Continue);
        }
        match planning {
            Some(Planning::Plan(filing)) => {
                item.plan = Some(Box::new(filing));
                item.review = None;
                Ok(StepOutcome::Finished)
            }
            Some(Planning::Review { reason: now, .. }) => {
                item.review = Some(now);
                Ok(StepOutcome::Continue)
            }
            None => Ok(StepOutcome::Continue),
        }
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
    /// Writes the confident fields over the empty and the guessed ones, reading the document
    /// again first: the provider call ran without the index. With `plan_again`
    /// the rules decide anew on the merged fields.
    fn write_and_plan(
        &self,
        item: &Item,
        confident: &[&ExtractedField],
        plan_again: bool,
        must_be_new: bool,
    ) -> Result<Option<Planning>, String> {
        let id = item
            .document
            .ok_or_else(|| "the file has no document".to_owned())?;
        let text = item.text.as_deref().unwrap_or_default();
        with_index_value(self.index.as_ref(), |index| {
            let Some(current) = index.document(id)? else {
                return Ok(None);
            };
            if must_be_new && current.status != DocumentStatus::New {
                return Ok(None);
            }
            let origins = index.field_origins(id)?;
            let known_before = current.fields.clone();
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
            if !plan_again {
                return Ok(None);
            }
            let sender_aliases = index.confirmed_sender_aliases()?;
            let manual_fields = index.field_origins(id)?.manual_fields();
            let kinds = Kinds::new(Arc::clone(index.kinds()), self.kind_language);
            let rules = index.rules()?;
            let location_path =
                relative_to_location(&item.path, std::slice::from_ref(&item.location.path));
            let plan_with = |fields: &DocumentFields| {
                plan(
                    &rules,
                    &PlanInput {
                        text,
                        fields,
                        path: Some(location_path),
                        location: Some(&item.location.path),
                        sender_aliases: &sender_aliases,
                        manual_fields: &manual_fields,
                        kinds: &kinds,
                    },
                )
            };
            let planning = plan_with(&merged);
            // A value the model read out of the text must not move the
            // document to a rule it did not match before.
            let rule_before = match plan_with(&known_before) {
                Planning::Plan(filing) => Some(filing.rule),
                Planning::Review { rule, .. } => rule,
            };
            if matches!(&planning, Planning::Plan(filing) if Some(filing.rule) != rule_before) {
                return Ok(None);
            }
            if let Planning::Plan(filing) = &planning {
                if filing.fields != merged {
                    index.set_document_fields(id, &filing.fields, |_| {
                        FieldOrigin::Rule(Some(filing.rule))
                    })?;
                }
                index.place_document(id)?;
            }
            Ok::<_, IndexError>(Some(planning))
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests;
