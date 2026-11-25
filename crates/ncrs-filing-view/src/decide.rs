//! What the rules make of the source, off the UI thread.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Task;
use ncrs_filing::chat_work::ReadProgress;
use ncrs_filing::sorting::{
    open_questions, plan, PlanError, PlanFiles, PlanRequest, Question, QuestionKind, ReadUnknown,
    RowState, SortingAccess, TargetState, WaitReason,
};
use ncrs_i18n::{Language, Msg};
use ncrs_index::{RuleId, TargetId};
use ncrs_intake::{canonical_path, SortSource, SortTarget};

use crate::message::Message;
use crate::state::{Row, Session};

/// One plan, or the open questions of a watched source as rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    pub rows: Vec<Row>,
    pub waiting: Vec<(TargetId, WaitReason)>,
    pub targets: Vec<SortTarget>,
    pub rule_names: HashMap<RuleId, String>,
    pub trash_can_restore: bool,
}

/// Decides for all files. With `read` the files the index does not know are
/// read for it, which takes a while.
pub fn decide(
    session: &mut Session,
    access: SortingAccess,
    read: bool,
    lang: Language,
) -> Task<Message> {
    session.planning = true;
    session.reading = read && session.once;
    session.stale = false;
    session.progress = Arc::default();
    let id = session.id;
    let once = session.once;
    let source = session.source.clone();
    let progress = Arc::clone(&session.progress);
    let stop = Arc::clone(&session.stop);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                if once {
                    decide_once(&access, &source, read, &progress, &stop)
                        .map_err(|err| plan_error_text(lang, &err))
                } else {
                    decide_questions(&access, &source)
                }
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()))
            .map(Box::new)
        },
        move |result| Message::Planned {
            session: id,
            read,
            result,
        },
    )
}

fn decided(
    access: &SortingAccess,
    rows: Vec<Row>,
    waiting: Vec<(TargetId, WaitReason)>,
) -> Result<Decided, String> {
    Ok(Decided {
        rows,
        waiting,
        targets: access.configured_targets(),
        rule_names: access.rule_names()?,
        trash_can_restore: access.trash_can_restore(),
    })
}

pub fn decide_once(
    access: &SortingAccess,
    source: &SortSource,
    read: bool,
    progress: &ReadProgress,
    stop: &AtomicBool,
) -> Result<Decided, PlanError> {
    let request = PlanRequest {
        source: source.clone(),
        files: PlanFiles::All,
        read: if read {
            ReadUnknown::Yes
        } else {
            ReadUnknown::No
        },
    };
    let plan = plan(access, &request, progress, Some(stop))?;
    let rows = plan.rows.into_iter().map(Row::from).collect();
    decided(access, rows, plan.waiting).map_err(PlanError::Index)
}

pub fn decide_questions(access: &SortingAccess, source: &SortSource) -> Result<Decided, String> {
    let root = canonical_path(&source.path).unwrap_or_else(|_| source.path.clone());
    let mut rows: Vec<Row> = open_questions(access, source)?
        .into_iter()
        .map(|question| question_row(&root, question))
        .collect();
    rows.sort_by(|first, second| first.relative.cmp(&second.relative));
    let waiting = source
        .targets
        .iter()
        .filter_map(|id| match access.target_state(id) {
            TargetState::Ready => None,
            TargetState::Waiting(reason) => Some((id.clone(), reason)),
        })
        .collect();
    decided(access, rows, waiting)
}

fn question_row(root: &Path, question: Question) -> Row {
    Row {
        relative: question
            .path
            .strip_prefix(root)
            .unwrap_or(&question.path)
            .to_path_buf(),
        path: question.path,
        hash: None,
        document: Some(question.document),
        state: match question.kind {
            QuestionKind::NoText => RowState::NoText,
            QuestionKind::SeveralTargets(places) if places.is_empty() => RowState::NoRule,
            QuestionKind::SeveralTargets(places) => RowState::Several(places),
        },
    }
}

fn plan_error_text(lang: Language, err: &PlanError) -> String {
    match err {
        PlanError::Source(_) => lang.text(Msg::FilingViewErrorSource).to_owned(),
        PlanError::Filter(reason) | PlanError::Index(reason) => reason.clone(),
        PlanError::Stopped => String::new(),
    }
}
