//! An old chat opens with its history and goes on when the user writes in it.

mod common;

#[cfg(test)]
mod tests {
    use crate::common::Talk;
    use chrono::Local;
    use ncrs_hermes_chat::{ChatId, Entry, ListMessage, Message, Part};

    const OTHER: &str = r#"{"intent":"other"}"#;

    fn talk() -> Talk {
        let mut talk = Talk::in_dir(tempfile::tempdir().unwrap(), &["archive"], &["inbox"]);
        talk.state.use_chat_store(talk.path("chats"), Local::now());
        talk.open();
        talk
    }

    fn ids(talk: &Talk) -> Vec<ChatId> {
        talk.state
            .chats()
            .groups(Local::now().date_naive())
            .into_iter()
            .flat_map(|group| group.rows)
            .map(|row| row.id)
            .collect()
    }

    fn user_texts(talk: &Talk) -> Vec<String> {
        talk.state
            .thread()
            .iter()
            .filter_map(|entry| match entry {
                Entry::User(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// A chat with one sentence, then a second chat on top of it; the first
    /// is shown again.
    fn old_chat_shown() -> (Talk, ChatId) {
        let mut talk = talk();
        talk.type_text_understood("Erste Frage", OTHER);
        let old = ids(&talk).remove(0);
        talk.send(Message::NewChat);
        talk.send(Message::List(ListMessage::Show(old.clone())));
        assert!(talk.state.chats().is_read_only());
        (talk, old)
    }

    #[test]
    fn writing_in_an_old_chat_continues_it_under_the_same_id() {
        let (mut talk, old) = old_chat_shown();
        let before = user_texts(&talk);
        assert!(before.is_empty(), "the running conversation is another");

        talk.type_text_understood("Zweite Frage", OTHER);

        assert_eq!(talk.state.chats().live(), Some(&old));
        assert_eq!(user_texts(&talk), ["Erste Frage", "Zweite Frage"]);
        assert_eq!(ids(&talk), vec![old], "no second chat was made");
    }

    #[test]
    fn the_model_asks_about_the_new_sentence_of_the_continued_chat() {
        let (mut talk, _) = old_chat_shown();
        talk.type_text_understood("Zweite Frage", OTHER);
        let turns = talk.model.turns();
        assert_eq!(
            turns.last().map(|turn| turn.message.as_str()),
            Some("Zweite Frage")
        );
    }

    #[test]
    fn an_old_chat_brings_no_plan_or_buttons_back() {
        let (mut talk, _) = old_chat_shown();
        talk.type_text_understood("Zweite Frage", OTHER);
        assert!(talk.state.thread().iter().take(2).all(|entry| match entry {
            Entry::Hermes { parts, chips } => {
                chips.is_empty() && parts.iter().all(|part| matches!(part, Part::Text(_)))
            }
            _ => true,
        }));
    }
}
