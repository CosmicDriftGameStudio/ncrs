use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use ncrs_ai::Provider;
use ncrs_index::{DocumentStatus, Field, FieldOrigin, IndexError, ReviewReason};
use ncrs_intake::text_is_readable;
use ncrs_intake::{
    detect_recipient, detect_sender, kind_from_heading, plan, OwnIdentifiers, PlanInput, Planning,
    StabilityTracker, TextChain, TextError,
};
use ncrs_intake::{settle_location, take_in, IntakeError, Location, LocationMode, Outcome};

use crate::ai_step::AiStep;
use crate::step::{resolve_reviews_of, with_index_value, IndexAccess, Item, Step, StepOutcome};

pub const NO_INDEX: &str = "index not open";

/// What belongs to the user and never to a sender.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnDetails {
    /// The user's full name and the names of their companies.
    pub names: Vec<String>,
    pub ibans: Vec<String>,
    pub tax_ids: Vec<String>,
}

/// The steps every file passes, in order. Steps that classify documents (text,
/// OCR, rules, fields, filing) register before the review step. The AI step
/// is there only when a provider is given; without one the steps are intake,
/// text, rules, review.
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
        }),
        Box::new(TextStep {
            index: Arc::clone(&index),
            chain: text,
        }),
        Box::new(RulesStep {
            index: Arc::clone(&index),
            own: own.clone(),
        }),
    ];
    if let Some(provider) = ai {
        steps.push(Box::new(AiStep::new(
            Arc::clone(&index),
            provider,
            own.names,
        )));
    }
    steps.push(Box::new(ReviewStep { index }));
    steps
}

/// Takes the file into the index, once it has held still.
#[derive(Debug)]
struct IntakeStep {
    index: Arc<dyn IndexAccess>,
    tracker: Mutex<StabilityTracker>,
}

impl Step for IntakeStep {
    fn name(&self) -> &'static str {
        "intake"
    }

    fn run(&self, item: &mut Item, now: SystemTime) -> Result<StepOutcome, String> {
        let taken = with_index_value(self.index.as_ref(), |index| {
            let mut tracker = self.tracker.lock().unwrap_or_else(PoisonError::into_inner);
            let outcome = take_in(index, &item.location, &item.path, &mut tracker, now)?;
            // An inbox document taken in but never reviewed (the app quit in
            // between) goes on again, and so does an archive document whose
            // text was never read, for example for lack of the PDF library.
            let unreviewed = match outcome {
                Outcome::Unchanged(id) if item.force => {
                    // A filed document keeps its status even where it lies in
                    // an inbox location: setting it back would plan it, and
                    // so move it, a second time.
                    let filed = index
                        .document(id)?
                        .is_some_and(|document| document.status == DocumentStatus::Filed);
                    if item.location.mode == LocationMode::Inbox && !filed {
                        index.set_document_status(id, DocumentStatus::New)?;
                    }
                    true
                }
                Outcome::Unchanged(id) => match item.location.mode {
                    LocationMode::Inbox => index
                        .document(id)?
                        .is_some_and(|document| document.status == DocumentStatus::New),
                    LocationMode::Archive => {
                        text_is_readable(&item.path) && !index.has_document_text_row(id)?
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

    fn after_scan(&self, location: &Location) -> Result<(), String> {
        with_index_value(self.index.as_ref(), |index| {
            settle_location(index, location)
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map(|_report| ())
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
        item.text = match self.chain.extract(&item.path) {
            Ok(text) => text,
            Err(TextError::LibraryMissing { .. }) => return Ok(StepOutcome::Unchanged),
            Err(err) => return Err(err.to_string()),
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

/// Lets the rules decide where a new inbox document would be filed, and stores
/// the fields they could fill. Nothing is moved here: a plan only ends the
/// pipeline for the document, executing it is a later step. A document the
/// rules cannot place carries the reason on to the review step.
#[derive(Debug)]
struct RulesStep {
    index: Arc<dyn IndexAccess>,
    own: OwnDetails,
}

impl Step for RulesStep {
    fn name(&self) -> &'static str {
        "rules"
    }

    fn run(&self, item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        if item.location.mode == LocationMode::Archive {
            return Ok(StepOutcome::Continue);
        }
        let id = item
            .document
            .ok_or_else(|| "the file has no document".to_owned())?;
        let text = item.text.as_deref().unwrap_or_default();
        let planning = with_index_value(self.index.as_ref(), |index| {
            let Some(document) = index.document(id)? else {
                return Ok(None);
            };
            // Fields of a document someone is already looking at or has filed
            // are theirs; the rules do not overwrite them.
            if document.status != DocumentStatus::New {
                return Ok(None);
            }
            let rules = index.rules()?;
            let aliases = index.confirmed_sender_aliases()?;
            let detected_recipient = document
                .fields
                .recipient
                .is_none()
                .then(|| detect_recipient(text))
                .flatten();
            let detected_kind = document
                .fields
                .kind
                .is_none()
                .then(|| kind_from_heading(text))
                .flatten();
            let detection = if document.fields.sender.is_none() {
                let own = OwnIdentifiers::new(
                    self.own.names.iter().map(String::as_str).chain(
                        detected_recipient
                            .as_deref()
                            .or(document.fields.recipient.as_deref()),
                    ),
                    self.own.ibans.iter().map(String::as_str),
                    self.own.tax_ids.iter().map(String::as_str),
                );
                Some(detect_sender(text, &aliases, &index.sender_names()?, &own))
            } else {
                None
            };
            let detected_name = detection
                .as_ref()
                .and_then(|found| found.sender.as_ref())
                .map(|sender| sender.name.clone());
            let mut known = document.fields.clone();
            if detected_name.is_some() {
                known.sender.clone_from(&detected_name);
            }
            if detected_recipient.is_some() {
                known.recipient.clone_from(&detected_recipient);
            }
            if detected_kind.is_some() {
                known.kind.clone_from(&detected_kind);
            }
            // Offered even when the plan ends in review: the user confirms
            // them from the alias list, not from this document.
            for offer in detection.iter().flat_map(|found| &found.offers) {
                index.offer_sender_alias(&offer.sender, offer.kind, &offer.value)?;
            }
            let manual_fields = index.field_origins(id)?.manual_fields();
            let planning = plan(
                &rules,
                &PlanInput {
                    text,
                    fields: &known,
                    path: Some(&item.path),
                    sender_aliases: &aliases,
                    manual_fields: &manual_fields,
                },
            );
            let (found, rule_origin) = match &planning {
                Planning::Plan(filing) => (&filing.fields, FieldOrigin::Rule(Some(filing.rule))),
                Planning::Review { fields, rule, .. } => (
                    fields,
                    rule.map_or(FieldOrigin::Unknown, |rule_id| {
                        FieldOrigin::Rule(Some(rule_id))
                    }),
                ),
            };
            if *found != document.fields {
                let sender_was_detected = detected_name.is_some();
                index.set_document_fields(id, found, |field| match field {
                    Field::Sender if sender_was_detected => FieldOrigin::Rule(None),
                    Field::Recipient if found.recipient == detected_recipient => {
                        FieldOrigin::AddressBlock
                    }
                    Field::Kind if found.kind == detected_kind => FieldOrigin::Heading,
                    _ => rule_origin.clone(),
                })?;
            }
            if matches!(planning, Planning::Plan(_)) {
                index.set_document_status(id, DocumentStatus::Planned)?;
                resolve_reviews_of(index, id)?;
            }
            Ok::<_, IndexError>(Some(planning))
        })
        .ok_or_else(|| NO_INDEX.to_owned())?
        .map_err(|err| err.to_string())?;
        match planning {
            Some(Planning::Plan(filing)) => {
                item.plan = Some(Box::new(filing));
                Ok(StepOutcome::Finished)
            }
            Some(Planning::Review { reason, .. }) => {
                item.review = Some(reason);
                Ok(StepOutcome::Continue)
            }
            None => Ok(StepOutcome::Continue),
        }
    }
}

/// Every inbox document the rules did not place lands in the review list, with
/// the reason the rules gave. Archive documents never get a review entry.
#[derive(Debug)]
struct ReviewStep {
    index: Arc<dyn IndexAccess>,
}

impl Step for ReviewStep {
    fn name(&self) -> &'static str {
        "review"
    }

    fn run(&self, item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        if item.location.mode == LocationMode::Archive {
            return Ok(StepOutcome::Finished);
        }
        let id = item
            .document
            .ok_or_else(|| "the file has no document".to_owned())?;
        let reason = item.review.unwrap_or(ReviewReason::NoMatchingRule);
        let added = with_index_value(self.index.as_ref(), |index| {
            let needs_review = index
                .document(id)?
                .is_some_and(|document| document.status == DocumentStatus::New);
            if needs_review {
                match index.add_review(id, reason) {
                    // Processed again while its review was still open: the
                    // intake had set it back to new, the open review is the
                    // state it belongs in.
                    Err(IndexError::ReviewAlreadyOpen(_)) => {
                        index.set_document_status(id, DocumentStatus::InReview)?;
                    }
                    other => {
                        other?;
                    }
                }
            }
            Ok::<_, IndexError>(())
        })
        .ok_or_else(|| NO_INDEX.to_owned())?;
        added
            .map(|()| StepOutcome::Finished)
            .map_err(|err| err.to_string())
    }
}
