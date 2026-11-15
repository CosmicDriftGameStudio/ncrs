//! Picks the archive folder for documents out of a numbered list. The
//! instructions and the list are decoded once and kept as a state, and every
//! document goes back to that state and decodes only its own tokens.

use std::num::NonZeroU32;

use llama_cpp_2::context::session::LlamaStateSeqFlags;
use llama_cpp_2::llama_batch::LlamaBatch;

use super::{chat_threads, LocalModel, MAX_BATCH_TOKENS};
use crate::error::AiError;

const SYSTEM_PROMPT: &str = "Du legst ein Dokument in ein Ablagearchiv. Wähle den Ordner, in den es gehört, nach Inhalt: Objekt/Adresse, Absender, Thema. Nimm den genauesten Ordner: einen Unterordner, wenn das Dokument zu ihm passt. Antworte nur mit der Nummer; 0, wenn keiner passt.";
const ANSWER_START: &[u8] = b"<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n";
/// A number up to three digits.
const MAX_REPLY_TOKENS: usize = 8;
/// Room for one document; longer ones are cut.
const DOCUMENT_TOKENS: usize = 1500;

impl LocalModel {
    /// For each document the index of the folder it belongs in, or `None`
    /// when the model says none fits.
    pub fn choose_folders(
        &self,
        folders: &[(String, Vec<String>)],
        documents: &[String],
    ) -> Result<Vec<Option<usize>>, AiError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let vocab = self.model.vocab();
        let list: String = folders
            .iter()
            .enumerate()
            .map(|(index, (folder, examples))| {
                if examples.is_empty() {
                    format!("{}. {folder}\n", index + 1)
                } else {
                    format!("{}. {folder} (z. B. {})\n", index + 1, examples.join(", "))
                }
            })
            .collect();
        let head = format!(
            "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\nOrdner:\n{list}\nDokument:\n"
        );
        let prefix = vocab.tokenize(head.as_bytes(), vocab.should_add_bos(), true);
        let tail = vocab.tokenize(ANSWER_START, false, true);

        let train_limit = usize::try_from(self.model.n_ctx_train()).unwrap_or(usize::MAX);
        let fixed = prefix.len() + tail.len() + MAX_REPLY_TOKENS;
        if fixed + 1 > train_limit {
            return Err(AiError::Model("the folder list is too long"));
        }
        let room = DOCUMENT_TOKENS.min(train_limit - fixed);
        let context_tokens = fixed + room;
        let n_ctx = u32::try_from(context_tokens)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(AiError::Model("the prompt is too long"))?;
        let batch_tokens = n_ctx.get().min(MAX_BATCH_TOKENS);
        let batch_len = usize::try_from(batch_tokens).unwrap_or(1);

        let mut context = self.new_context(n_ctx, batch_tokens, chat_threads())?;
        let mut batch = LlamaBatch::new(batch_len, 1);
        self.decode_tokens(&mut context, &mut batch, &prefix, 0, batch_len)?;
        let after_list = context
            .state_seq_get(0, LlamaStateSeqFlags::empty())
            .map_err(|_refused| AiError::Model("the model could not keep the folder list"))?;

        let numbers: Vec<String> = (0..=folders.len()).map(|n| format!("\"{n}\"")).collect();
        let grammar = format!("root ::= {}", numbers.join(" | "));
        let mut chosen = Vec::with_capacity(documents.len());
        for document in documents {
            // The document is the user's text: special tokens in it stay plain text.
            let mut suffix = vocab.tokenize(document.as_bytes(), false, false);
            suffix.truncate(room);
            suffix.extend_from_slice(&tail);

            context.clear_kv_cache();
            context
                .state_seq_set(&after_list, 0)
                .map_err(|_refused| AiError::Model("the model could not start a document"))?;
            self.decode_tokens(&mut context, &mut batch, &suffix, prefix.len(), batch_len)?;
            let raw = self.sample_reply(
                &mut context,
                &mut batch,
                &grammar,
                prefix.len() + suffix.len(),
                MAX_REPLY_TOKENS,
            )?;
            let number: usize = raw.trim().parse().map_err(|_not_a_number| {
                AiError::Model("the model did not answer with a number")
            })?;
            chosen.push(number.checked_sub(1));
        }
        Ok(chosen)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::print_stdout,
        clippy::panic,
        clippy::shadow_reuse,
        clippy::shadow_unrelated,
        clippy::indexing_slicing,
        clippy::cast_precision_loss
    )]

    use std::path::PathBuf;
    use std::time::Instant;

    use jsonc_parser::{parse_to_ast, CollectOptions, ParseOptions};

    use super::super::tests::model;

    fn string_field(object: &jsonc_parser::ast::Object<'_>, name: &str) -> Option<String> {
        match &object.get(name)?.value {
            jsonc_parser::ast::Value::StringLit(text) => Some(text.value.to_string()),
            _ => None,
        }
    }

    fn is_year_folder(name: &str) -> bool {
        name.len() == 4
            && (name.starts_with("19") || name.starts_with("20"))
            && name.bytes().all(|byte| byte.is_ascii_digit())
    }

    fn files_in(folder: &std::path::Path) -> Vec<std::fs::DirEntry> {
        let mut entries: Vec<_> = std::fs::read_dir(folder)
            .map(|read| read.flatten().collect())
            .unwrap_or_default();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        entries
    }

    /// Up to three file names directly in the folder or in its year folders.
    fn example_names(folder: &std::path::Path) -> Vec<String> {
        let mut names = Vec::new();
        for entry in files_in(folder) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_file() {
                names.push(name);
            } else if is_year_folder(&name) {
                names.extend(
                    files_in(&entry.path())
                        .into_iter()
                        .filter(|inner| inner.path().is_file())
                        .map(|inner| inner.file_name().to_string_lossy().into_owned()),
                );
            }
        }
        names.retain(|name| !name.starts_with('.'));
        names.truncate(3);
        names
            .iter()
            .map(|name| name.chars().take(40).collect())
            .collect()
    }

    #[test]
    #[ignore = "needs the AI model and NCRS_EVAL_FOLDER_CHOICE"]
    fn folder_choice_eval() {
        let Some(dir) = std::env::var_os("NCRS_EVAL_FOLDER_CHOICE").map(PathBuf::from) else {
            println!("NCRS_EVAL_FOLDER_CHOICE is not set");
            return;
        };
        let folders: Vec<String> = std::fs::read_to_string(dir.join("folders.txt"))
            .unwrap()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_owned)
            .collect();
        let archive = std::env::var_os("NCRS_EVAL_ARCHIVE").map(PathBuf::from);
        let choices: Vec<(String, Vec<String>)> = folders
            .iter()
            .map(|folder| {
                let examples = archive
                    .as_ref()
                    .map(|root| example_names(&root.join(folder)))
                    .unwrap_or_default();
                (folder.clone(), examples)
            })
            .collect();
        let expected = std::fs::read_to_string(dir.join("expected.tsv")).unwrap();
        let scans = std::fs::read_to_string(dir.join("scans.jsonl")).unwrap();
        let local = model();

        let mut names = Vec::new();
        let mut documents = Vec::new();
        for line in scans.lines().filter(|line| !line.trim().is_empty()) {
            let parsed =
                parse_to_ast(line, &CollectOptions::default(), &ParseOptions::default()).unwrap();
            let object = parsed.value.unwrap();
            let jsonc_parser::ast::Value::Object(object) = object else {
                panic!("a scan is an object");
            };
            let name = string_field(&object, "name").unwrap();
            let document = format!(
                "Absender: {}\nArt: {}\nDatum: {}\n\n{}",
                string_field(&object, "sender").unwrap_or_default(),
                string_field(&object, "kind").unwrap_or_default(),
                string_field(&object, "date").unwrap_or_default(),
                string_field(&object, "text").unwrap_or_default(),
            );
            names.push(name);
            documents.push(document);
        }

        let started = Instant::now();
        let picks = local.choose_folders(&choices, &documents).unwrap();
        let seconds = started.elapsed().as_secs_f64();
        let (mut exact, mut area, mut none, mut total) = (0, 0, 0, 0);
        for (name, chosen) in names.iter().zip(picks) {
            let allowed: Vec<&str> = expected
                .lines()
                .find_map(|row| row.split_once('\t').filter(|(scan, _)| *scan == name))
                .map(|(_, folders)| folders.split(" | ").collect())
                .unwrap_or_default();

            let chosen_name = chosen.map(|index| folders[index].as_str());
            let is_exact = chosen_name.is_some_and(|folder| allowed.contains(&folder));
            let is_area = chosen_name.is_some_and(|folder| {
                allowed
                    .iter()
                    .any(|path| path.split('/').next() == folder.split('/').next())
            });
            total += 1;
            exact += usize::from(is_exact);
            area += usize::from(is_area);
            none += usize::from(chosen.is_none());
            println!(
                "{name} | {} | {} | {}",
                chosen_name.unwrap_or("-"),
                allowed.join(" | "),
                if is_exact { "ja" } else { "nein" }
            );
        }
        println!(
            "exakt {exact}/{total}, Bereich {area}/{total}, keiner {none}/{total}, {:.1} s je Scan",
            seconds / total.max(1) as f64
        );
    }

    #[test]
    #[ignore = "loads the real local model from NCRS_AI_MODELS_DIR"]
    fn quitting_does_not_wait_for_a_long_folder_choice() {
        use std::num::NonZeroU32;
        use std::sync::Arc;
        use std::time::Duration;

        use crate::chat::ChatModel as _;
        use crate::model_file::LOCAL_MODEL;

        let lazy = Arc::new(crate::LazyLocalModel::new(
            super::super::tests::models_dir().join(LOCAL_MODEL.name),
            LOCAL_MODEL,
            NonZeroU32::new(4).unwrap(),
            NonZeroU32::new(2048).unwrap(),
        ));
        let folders = vec![
            ("Bank".to_owned(), Vec::new()),
            ("Steuer".to_owned(), Vec::new()),
        ];
        let documents = vec!["Absender: Bank\nArt: Kontoauszug\n\nSaldo ".repeat(100); 200];
        let asking = Arc::clone(&lazy);
        let choosing = std::thread::spawn(move || {
            asking.choose_folders(&folders, &documents, crate::FolderChoiceLane::Waiting)
        });
        // The model loads and works through the first documents.
        std::thread::sleep(Duration::from_secs(20));
        assert!(!choosing.is_finished(), "200 documents take minutes");

        let started = Instant::now();
        lazy.shutdown();
        let waited = started.elapsed();
        assert!(matches!(choosing.join(), Ok(Err(_))));
        println!("quit waited {} s", waited.as_secs_f32());
        assert!(
            waited < Duration::from_secs(10),
            "quit waited {} s",
            waited.as_secs_f32()
        );
    }
}
