//! Which tags the file panels show after a file name: the setting and the
//! choice from the tags a document has. Pure; the panel draws the result.

use std::collections::BTreeMap;

use ncrs_index::{tag_name, FileTags, Kinds};

/// Most chips after one name; the rest is counted.
pub const MOST_CHIPS: usize = 3;

/// A kind of tags the panel can show, as it is spelled in the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagGroup {
    Kind,
    Recipient,
    Sender,
    Year,
    Own,
}

impl TagGroup {
    /// In the order the chips appear.
    pub const ALL: [Self; 5] = [
        Self::Kind,
        Self::Recipient,
        Self::Sender,
        Self::Year,
        Self::Own,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Kind => "kind",
            Self::Recipient => "recipient",
            Self::Sender => "sender",
            Self::Year => "year",
            Self::Own => "own",
        }
    }
}

/// The `[file_list_tags]` setting as it is in effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListTags {
    pub show: bool,
    /// The tags of these groups show unless a tag has an exception.
    pub groups: Vec<TagGroup>,
}

impl Default for FileListTags {
    fn default() -> Self {
        Self {
            show: true,
            groups: vec![TagGroup::Kind, TagGroup::Own],
        }
    }
}

impl FileListTags {
    pub fn has_group(&self, group: TagGroup) -> bool {
        self.groups.contains(&group)
    }

    /// The setting with the group switched on or off, in the fixed order.
    pub fn with_group(&self, group: TagGroup, on: bool) -> Self {
        let groups = TagGroup::ALL
            .into_iter()
            .filter(|known| {
                if *known == group {
                    on
                } else {
                    self.has_group(*known)
                }
            })
            .collect();
        Self {
            groups,
            ..self.clone()
        }
    }
}

/// One chip after a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChip {
    pub tag: String,
    /// The user set it; the others were read from the document.
    pub own: bool,
}

/// What follows a file name: at most [`MOST_CHIPS`] chips and how many more
/// there are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShownTags {
    pub chips: Vec<FileChip>,
    pub more: usize,
}

fn worded(group: TagGroup, tag: &str, kinds: &Kinds) -> String {
    match group {
        TagGroup::Kind => kinds.kind_tag_word(tag),
        TagGroup::Recipient | TagGroup::Sender | TagGroup::Year | TagGroup::Own => tag.to_owned(),
    }
}

/// The tags that differ from their group's default, by tag name: `true` shows
/// the tag although its group is off, `false` hides it although the group is on.
pub type TagExceptions = BTreeMap<String, bool>;

/// Kind, recipient, sender, year, then the own tags. A tag shows when its
/// exception says so, else when its group is on. A tag that is both read and
/// set shows once, as an own one. A known kind is worded in the language of
/// `kinds`.
pub fn shown_tags(
    tags: &FileTags,
    setting: &FileListTags,
    exceptions: &TagExceptions,
    kinds: &Kinds,
) -> ShownTags {
    if !setting.show {
        return ShownTags::default();
    }
    let wanted = |group: TagGroup, tag: &str| {
        exceptions
            .get(&tag_name(tag))
            .copied()
            .unwrap_or_else(|| setting.has_group(group))
    };
    let own_shown: Vec<&String> = tags
        .own
        .iter()
        .filter(|tag| wanted(TagGroup::Own, tag))
        .collect();
    let automatic = [
        (TagGroup::Kind, tags.kind.as_ref()),
        (TagGroup::Recipient, tags.recipient.as_ref()),
        (TagGroup::Sender, tags.sender.as_ref()),
        (TagGroup::Year, tags.year.as_ref()),
    ];
    let mut candidates: Vec<FileChip> = Vec::new();
    for (group, automatic_tag) in automatic {
        let Some(tag) = automatic_tag.filter(|tag| wanted(group, tag)) else {
            continue;
        };
        let shown = worded(group, tag, kinds);
        let set_by_user = own_shown
            .iter()
            .any(|own| worded(group, own, kinds) == shown);
        let seen = candidates.iter().any(|chip| chip.tag == shown);
        if !set_by_user && !seen {
            candidates.push(FileChip {
                tag: shown,
                own: false,
            });
        }
    }
    candidates.extend(own_shown.into_iter().map(|tag| FileChip {
        tag: tag.clone(),
        own: true,
    }));
    let more = candidates.len().saturating_sub(MOST_CHIPS);
    candidates.truncate(MOST_CHIPS);
    ShownTags {
        chips: candidates,
        more,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncrs_index::KindLanguage;

    const GERMAN: KindLanguage = KindLanguage::German;

    fn read_document() -> FileTags {
        FileTags {
            kind: Some("rechnung".to_owned()),
            recipient: Some("firma-a-gmbh".to_owned()),
            sender: Some("telekom".to_owned()),
            year: Some("2026".to_owned()),
            own: vec!["steuer".to_owned()],
        }
    }

    fn texts(shown: &ShownTags) -> Vec<(&str, bool)> {
        shown
            .chips
            .iter()
            .map(|chip| (chip.tag.as_str(), chip.own))
            .collect()
    }

    #[test]
    fn by_default_the_kind_and_the_own_tags_show() {
        let shown = shown_tags(
            &read_document(),
            &FileListTags::default(),
            &TagExceptions::new(),
            &Kinds::built_in(GERMAN),
        );

        assert_eq!(texts(&shown), [("rechnung", false), ("steuer", true)]);
        assert_eq!(shown.more, 0);
    }

    #[test]
    fn the_kind_shows_as_the_word_of_the_language_and_a_set_twin_hides_it() {
        let mut tags = read_document();
        tags.kind = Some("invoice".to_owned());

        let english = shown_tags(
            &tags,
            &FileListTags::default(),
            &TagExceptions::new(),
            &Kinds::built_in(KindLanguage::English),
        );
        let german = shown_tags(
            &tags,
            &FileListTags::default(),
            &TagExceptions::new(),
            &Kinds::built_in(GERMAN),
        );
        tags.own = vec!["rechnung".to_owned()];
        let twin = shown_tags(
            &tags,
            &FileListTags::default(),
            &TagExceptions::new(),
            &Kinds::built_in(GERMAN),
        );

        assert_eq!(texts(&english), [("invoice", false), ("steuer", true)]);
        assert_eq!(texts(&german), [("rechnung", false), ("steuer", true)]);
        assert_eq!(texts(&twin), [("rechnung", true)]);
    }

    #[test]
    fn the_groups_come_in_the_fixed_order_and_three_chips_at_most_are_shown() {
        let setting = FileListTags {
            groups: TagGroup::ALL.to_vec(),
            ..FileListTags::default()
        };

        let shown = shown_tags(
            &read_document(),
            &setting,
            &TagExceptions::new(),
            &Kinds::built_in(GERMAN),
        );

        assert_eq!(
            texts(&shown),
            [
                ("rechnung", false),
                ("firma-a-gmbh", false),
                ("telekom", false)
            ]
        );
        assert_eq!(shown.more, 2);
    }

    #[test]
    fn a_tag_that_is_read_and_set_shows_once_as_an_own_one() {
        let mut tags = read_document();
        tags.own = vec!["rechnung".to_owned()];

        let shown = shown_tags(
            &tags,
            &FileListTags::default(),
            &TagExceptions::new(),
            &Kinds::built_in(GERMAN),
        );

        assert_eq!(texts(&shown), [("rechnung", true)]);
    }

    #[test]
    fn a_tag_that_is_set_stays_automatic_while_the_own_group_is_off() {
        let mut tags = read_document();
        tags.own = vec!["rechnung".to_owned()];
        let setting = FileListTags::default().with_group(TagGroup::Own, false);

        assert_eq!(
            texts(&shown_tags(
                &tags,
                &setting,
                &TagExceptions::new(),
                &Kinds::built_in(GERMAN)
            )),
            [("rechnung", false)]
        );
    }

    #[test]
    fn an_exception_beats_the_default_of_its_group_in_both_directions() {
        let exceptions = TagExceptions::from([
            ("telekom".to_owned(), true),
            ("rechnung".to_owned(), false),
            ("steuer".to_owned(), false),
        ]);

        let shown = shown_tags(
            &read_document(),
            &FileListTags::default(),
            &exceptions,
            &Kinds::built_in(GERMAN),
        );

        assert_eq!(texts(&shown), [("telekom", false)]);
    }

    #[test]
    fn the_exception_of_a_kind_is_found_by_the_tag_in_the_index_in_every_language() {
        let mut tags = read_document();
        tags.kind = Some("invoice".to_owned());
        let exceptions = TagExceptions::from([("invoice".to_owned(), false)]);

        let shown = shown_tags(
            &tags,
            &FileListTags::default(),
            &exceptions,
            &Kinds::built_in(GERMAN),
        );

        assert_eq!(texts(&shown), [("steuer", true)]);
    }

    #[test]
    fn nothing_shows_when_switched_off_or_for_a_file_without_tags() {
        let off = FileListTags {
            show: false,
            ..FileListTags::default()
        };

        assert_eq!(
            shown_tags(
                &read_document(),
                &off,
                &TagExceptions::new(),
                &Kinds::built_in(GERMAN)
            ),
            ShownTags::default()
        );
        assert_eq!(
            shown_tags(
                &FileTags::default(),
                &FileListTags::default(),
                &TagExceptions::new(),
                &Kinds::built_in(GERMAN)
            ),
            ShownTags::default()
        );
    }

    #[test]
    fn a_group_is_switched_on_or_off_without_changing_the_order() {
        let setting = FileListTags::default().with_group(TagGroup::Sender, true);

        assert_eq!(
            setting.groups,
            [TagGroup::Kind, TagGroup::Sender, TagGroup::Own]
        );
        assert_eq!(
            setting.with_group(TagGroup::Kind, false).groups,
            [TagGroup::Sender, TagGroup::Own]
        );
    }
}
