//! The chat and the rule questions of Hermes, asked of the same server as the
//! documents. The prompts and the parsers are the ones the local model uses;
//! only the way the answer is kept to JSON differs: a local model is held to a
//! grammar, a server is asked for a JSON object and a reply that does not
//! parse is the existing parse error.

use ncrs_index::{DocumentFields, Kinds, RuleSpec};

use super::*;
use crate::chat::{
    chat_user_message, parse_chat_reply, ChatIntent, ChatModel, ChatTurn, CHAT_SYSTEM_PROMPT,
};
use crate::rules::{
    annotated_paths, parse_generate, parse_levels, parse_revision, prefix_text, repair_note_for,
    revise_question, ContextDocument, Levels, Mismatch, Revision, RuleRequest, RECOGNIZE_QUESTION,
};
use crate::status::{Compute, LocalModelStatus};

/// The rule prefix is a chat template for a local model: the system message
/// sits between these two markers.
const SYSTEM_OPEN: &str = "<|im_start|>system\n";
const SYSTEM_CLOSE: &str = "<|im_end|>\n<|im_start|>user\n";

fn system_of(prefix: &str) -> &str {
    prefix
        .strip_prefix(SYSTEM_OPEN)
        .and_then(|rest| rest.strip_suffix(SYSTEM_CLOSE))
        .unwrap_or(prefix)
}

fn documents_of(files: &[(String, DocumentFields)]) -> Vec<ContextDocument> {
    files
        .iter()
        .map(|(path, fields)| ContextDocument {
            fields: fields.clone(),
            folder: path.rsplit_once('/').map(|(folder, _)| folder.to_owned()),
        })
        .collect()
}

impl OpenAiCompatible {
    /// One question, answered as a JSON object. The usage is logged.
    fn ask_for_json(&self, system: &str, user: &str) -> Result<String, AiError> {
        let mut body = Json::object([
            ("model", Json::text(&self.model)),
            (
                "messages",
                Json::List(vec![
                    Json::object([
                        ("role", Json::text("system")),
                        ("content", Json::text(system)),
                    ]),
                    Json::object([("role", Json::text("user")), ("content", Json::text(user))]),
                ]),
            ),
            (
                "response_format",
                Json::object([("type", Json::text("json_object"))]),
            ),
        ]);
        self.include_usage(&mut body);
        let reply = self.post(&body.pretty())?;
        self.log_usage(&reply);
        message_content(&reply)
    }

    fn answer_rule_request(&self, request: &RuleRequest) -> Result<String, AiError> {
        self.ask_for_json(system_of(&request.prefix), &request.task)
    }

    fn recognize_with(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
        question: &str,
    ) -> Result<Levels, AiError> {
        let prefix = prefix_text(&documents_of(files), kinds);
        let paths = annotated_paths(files, kinds);
        let request = RuleRequest::recognize(&prefix, question, &paths, folder_depth);
        parse_levels(&self.answer_rule_request(&request)?)
    }
}

impl ChatModel for OpenAiCompatible {
    fn understand(&self, turn: &ChatTurn) -> Result<ChatIntent, AiError> {
        let raw = self.ask_for_json(CHAT_SYSTEM_PROMPT, &chat_user_message(turn)?)?;
        parse_chat_reply(&raw, turn)
    }

    fn generate_rule(
        &self,
        description: &str,
        documents: &[ContextDocument],
        kinds: &Kinds,
    ) -> Result<RuleSpec, AiError> {
        let prefix = prefix_text(documents, kinds);
        let request = RuleRequest::generate(&prefix, description);
        parse_generate(&self.answer_rule_request(&request)?)
    }

    fn recognize_levels(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
    ) -> Result<Levels, AiError> {
        self.recognize_with(files, kinds, folder_depth, RECOGNIZE_QUESTION)
    }

    fn recognize_levels_again(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
        previous: &Levels,
        misfits: &[Mismatch],
    ) -> Result<Levels, AiError> {
        let note = repair_note_for(previous, misfits);
        self.recognize_with(
            files,
            kinds,
            folder_depth,
            &format!("{RECOGNIZE_QUESTION}\n{note}"),
        )
    }

    fn revise_levels(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        previous: &Levels,
        sentence: &str,
    ) -> Result<Revision, AiError> {
        let prefix = prefix_text(&documents_of(files), kinds);
        let request = RuleRequest::revise(&prefix, &revise_question(previous, sentence));
        parse_revision(&self.answer_rule_request(&request)?)
    }

    /// A server is there when it is asked; nothing is loaded here.
    fn status(&self) -> LocalModelStatus {
        LocalModelStatus::Loaded {
            name: self.model.clone(),
            memory_bytes: 0,
            compute: Compute::Cpu,
        }
    }

    fn refresh(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_message_is_what_lies_between_the_template_markers() {
        let prefix = prefix_text(&[], &Kinds::default());
        let system = system_of(&prefix);
        assert!(!system.contains("<|im_"));
        assert!(!system.is_empty());
        assert_eq!(system_of("plain"), "plain");
    }

    #[test]
    fn a_documents_folder_is_what_comes_before_the_last_slash() {
        let files = [
            ("Post/2021/a.pdf".to_owned(), DocumentFields::default()),
            ("b.pdf".to_owned(), DocumentFields::default()),
        ];
        let folders: Vec<Option<String>> = documents_of(&files)
            .into_iter()
            .map(|document| document.folder)
            .collect();
        assert_eq!(folders, vec![Some("Post/2021".to_owned()), None]);
    }
}
