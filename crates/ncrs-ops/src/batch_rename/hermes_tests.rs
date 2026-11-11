//! When the file names of a folder go to a model: on opening for a model on
//! this computer, on a click for one on another computer.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use iced::Task;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action as RuntimeAction};
use ncrs_ai::{ChatModel, LocalModelStatus, ScriptedChatModel};
use ncrs_core::{FileEntry, PanelSide};
use ncrs_fs::remote::session::Pool;
use ncrs_fs::remote::KnownHostsFiles;
use ncrs_i18n::Language;

use super::flow::{open_batch_rename, update_batch_rename, BatchRenameRequest};
use super::state::{BatchRenameMessage, HermesAsk, Token};
use crate::ctx::{FlowCtx, NamesModel};
use crate::message::OpsMessage;
use crate::state::Ops;

const OTHER_REPLY: &str = r#"{"intent":"other"}"#;

fn fake_model() -> Arc<ScriptedChatModel> {
    Arc::new(ScriptedChatModel::new(
        LocalModelStatus::NotDownloaded,
        [Ok(OTHER_REPLY), Ok(OTHER_REPLY)],
    ))
}

fn with_model<R>(model: NamesModel, step: impl FnOnce(&FlowCtx<'_>) -> R) -> R {
    let trash = ncrs_fs::system_trash(None);
    let sandbox = ncrs_fs::SandboxAccess::inactive();
    let remote = Arc::new(Pool::over_tcp(KnownHostsFiles::default()));
    step(&FlowCtx {
        lang: Language::default(),
        trash: &trash,
        jobs_busy: false,
        start_blocked: false,
        transfer_dialog_open: false,
        has_job_error: false,
        sandbox: &sandbox,
        source_side: PanelSide::Left,
        left_archive: None,
        right_archive: None,
        left_server: None,
        right_server: None,
        remote: &remote,
        permanent_delete_key: None,
        restore_last_key: None,
        index: None,
        model,
    })
}

/// Runs what the task starts to its end and gives the messages it reported.
async fn settle(task: Task<OpsMessage>) -> Vec<OpsMessage> {
    let mut reported = Vec::new();
    if let Some(mut stream) = task::into_stream(task) {
        while let Some(action) = stream.next().await {
            if let RuntimeAction::Output(message) = action {
                reported.push(message);
            }
        }
    }
    reported
}

fn request(folder: &std::path::Path) -> BatchRenameRequest {
    let names = ["a.pdf", "b.pdf"];
    for name in names {
        std::fs::write(folder.join(name), name).unwrap();
    }
    BatchRenameRequest {
        side: PanelSide::Left,
        folder: folder.to_path_buf(),
        entries: names
            .iter()
            .map(|name| FileEntry::from_path(&folder.join(name)).unwrap())
            .collect(),
        folder_names: names.iter().map(|name| (*name).to_owned()).collect(),
    }
}

async fn open(model: &NamesModel, folder: &std::path::Path) -> Ops {
    let mut ops = Ops::default();
    let (task, _) = with_model(model.clone(), |ctx| {
        open_batch_rename(&mut ops, request(folder), ctx)
    });
    settle(task).await;
    ops
}

async fn send(ops: &mut Ops, model: &NamesModel, message: BatchRenameMessage) {
    let (task, _) = with_model(model.clone(), |ctx| update_batch_rename(ops, message, ctx));
    settle(task).await;
}

fn names_asked(model: &ScriptedChatModel) -> Vec<Vec<String>> {
    model
        .turns()
        .into_iter()
        .map(|turn| turn.question.expect("the names go as a question").options)
        .collect()
}

#[tokio::test]
async fn a_model_on_this_computer_is_asked_when_the_dialog_opens() {
    let folder = tempfile::tempdir().unwrap();
    let model = fake_model();
    let names_model = NamesModel::Local(Arc::clone(&model) as Arc<dyn ChatModel>);

    let ops = open(&names_model, folder.path()).await;

    assert_eq!(names_asked(&model), [["a.pdf", "b.pdf"]]);
    assert_eq!(ops.batch_rename.unwrap().ask, HermesAsk::Unavailable);
}

#[tokio::test]
async fn a_model_on_another_computer_is_not_asked_when_the_dialog_opens() {
    let folder = tempfile::tempdir().unwrap();
    let model = fake_model();
    let names_model = NamesModel::Remote(Arc::clone(&model) as Arc<dyn ChatModel>);

    let ops = open(&names_model, folder.path()).await;

    assert!(model.turns().is_empty());
    assert_eq!(ops.batch_rename.unwrap().ask, HermesAsk::Offered);
}

#[tokio::test]
async fn the_click_sends_the_names_to_a_model_on_another_computer_once() {
    let folder = tempfile::tempdir().unwrap();
    let model = fake_model();
    let names_model = NamesModel::Remote(Arc::clone(&model) as Arc<dyn ChatModel>);
    let mut ops = open(&names_model, folder.path()).await;

    send(&mut ops, &names_model, BatchRenameMessage::AskHermes).await;
    send(&mut ops, &names_model, BatchRenameMessage::AskHermes).await;

    assert_eq!(names_asked(&model), [["a.pdf", "b.pdf"]]);
    assert_eq!(ops.batch_rename.unwrap().ask, HermesAsk::Asked);
}

#[tokio::test]
async fn nothing_but_the_click_sends_names_to_a_model_on_another_computer() {
    let folder = tempfile::tempdir().unwrap();
    let model = fake_model();
    let names_model = NamesModel::Remote(Arc::clone(&model) as Arc<dyn ChatModel>);
    let mut ops = open(&names_model, folder.path()).await;

    for message in [
        BatchRenameMessage::PatternInput("new{extension}".to_owned()),
        BatchRenameMessage::Insert(Token::Counter),
        BatchRenameMessage::FindInput("a".to_owned()),
        BatchRenameMessage::ToggleRegex,
        BatchRenameMessage::UseHermes,
        BatchRenameMessage::Cancel,
        BatchRenameMessage::AskHermes,
    ] {
        send(&mut ops, &names_model, message).await;
    }

    assert!(model.turns().is_empty());
    assert!(ops.batch_rename.is_none());
}

#[tokio::test]
async fn without_a_model_nothing_is_offered_and_the_click_does_nothing() {
    let folder = tempfile::tempdir().unwrap();
    let mut ops = open(&NamesModel::None, folder.path()).await;
    assert_eq!(
        ops.batch_rename.as_ref().unwrap().ask,
        HermesAsk::Unavailable
    );

    send(&mut ops, &NamesModel::None, BatchRenameMessage::AskHermes).await;

    assert_eq!(ops.batch_rename.unwrap().ask, HermesAsk::Unavailable);
}
