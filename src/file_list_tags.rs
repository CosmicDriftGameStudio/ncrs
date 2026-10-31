//! Which tags the file panels show after a file name: the setting and the
//! choice from the tags a document has. Pure; the panel draws the result.

use ncrs_index::{FileTags, Kinds};

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
    pub groups: Vec<TagGroup>,
    /// When there are any, only tags that match one of them are shown, from
    /// every group.
    pub only: Vec<String>,
}

impl Default for FileListTags {
    fn default() -> Self {
        Self {
            show: true,
            groups: vec![TagGroup::Kind, TagGroup::Own],
            only: Vec::new(),
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

/// Kind, recipient, sender, year, then the own tags. A tag that is both read
/// and set shows once, as an own one. A known kind is worded in the language
/// of `kinds`.
pub fn shown_tags(tags: &FileTags, setting: &FileListTags, kinds: &Kinds) -> ShownTags {
    if !setting.show {
        return ShownTags::default();
    }
    let narrowed = !setting.only.is_empty();
    let wanted = |group: TagGroup| narrowed || setting.has_group(group);
    let automatic = [
        (TagGroup::Kind, tags.kind.as_ref()),
        (TagGroup::Recipient, tags.recipient.as_ref()),
        (TagGroup::Sender, tags.sender.as_ref()),
        (TagGroup::Year, tags.year.as_ref()),
    ];
    let own_listed = wanted(TagGroup::Own);
    let mut candidates: Vec<FileChip> = Vec::new();
    for (group, automatic_tag) in automatic {
        let Some(tag) = automatic_tag.filter(|_| wanted(group)) else {
            continue;
        };
        let shown = worded(group, tag, kinds);
        let set_by_user = own_listed
            && tags
                .own
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
    if own_listed {
        candidates.extend(tags.own.iter().map(|tag| FileChip {
            tag: tag.clone(),
            own: true,
        }));
    }
    if narrowed {
        candidates.retain(|chip| {
            setting
                .only
                .iter()
                .any(|entry| kinds.catalog.tag_matches(&chip.tag, entry))
        });
    }
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
            &Kinds::built_in(KindLanguage::English),
        );
        let german = shown_tags(&tags, &FileListTags::default(), &Kinds::built_in(GERMAN));
        tags.own = vec!["rechnung".to_owned()];
        let twin = shown_tags(&tags, &FileListTags::default(), &Kinds::built_in(GERMAN));

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

        let shown = shown_tags(&read_document(), &setting, &Kinds::built_in(GERMAN));

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

        let shown = shown_tags(&tags, &FileListTags::default(), &Kinds::built_in(GERMAN));

        assert_eq!(texts(&shown), [("rechnung", true)]);
    }

    #[test]
    fn a_tag_that_is_set_stays_automatic_while_the_own_group_is_off() {
        let mut tags = read_document();
        tags.own = vec!["rechnung".to_owned()];
        let setting = FileListTags::default().with_group(TagGroup::Own, false);

        assert_eq!(
            texts(&shown_tags(&tags, &setting, &Kinds::built_in(GERMAN))),
            [("rechnung", false)]
        );
    }

    #[test]
    fn only_these_replaces_the_groups_and_matches_like_a_search() {
        let setting = FileListTags {
            groups: vec![TagGroup::Own],
            only: vec!["firma-a".to_owned(), "2026".to_owned()],
            ..FileListTags::default()
        };

        let shown = shown_tags(&read_document(), &setting, &Kinds::built_in(GERMAN));

        assert_eq!(texts(&shown), [("firma-a-gmbh", false), ("2026", false)]);
    }

    #[test]
    fn nothing_shows_when_switched_off_or_for_a_file_without_tags() {
        let off = FileListTags {
            show: false,
            ..FileListTags::default()
        };

        assert_eq!(
            shown_tags(&read_document(), &off, &Kinds::built_in(GERMAN)),
            ShownTags::default()
        );
        assert_eq!(
            shown_tags(
                &FileTags::default(),
                &FileListTags::default(),
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
