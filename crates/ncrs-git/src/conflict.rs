//! Conflict markers in a file: finding the blocks and rewriting them. Pure text, no git.

use std::ops::Range;

const MARKER_LENGTH: usize = 7;

/// One `<<<<<<<` .. `>>>>>>>` stretch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictBlock {
    /// 1-based line of the opening marker.
    pub start_line: u32,
    pub ours_label: String,
    pub theirs_label: String,
    /// The lines of the checked-out side (the commit being built on).
    pub ours: String,
    /// The common ancestor, present with `merge.conflictStyle=diff3` or `zdiff3`.
    pub base: Option<String>,
    /// The lines of the side being merged in (while rebasing: the commit being replayed).
    pub theirs: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    Ours,
    Theirs,
    /// Ours, then theirs.
    Both,
}

fn marker_rest(line: &str, marker: char) -> Option<&str> {
    let text = line.trim_end_matches(['\n', '\r']);
    let rest = text.strip_prefix(&marker.to_string().repeat(MARKER_LENGTH))?;
    (rest.is_empty() || rest.starts_with(' ')).then(|| rest.trim())
}

struct Found {
    lines: Range<usize>,
    block: ConflictBlock,
}

#[derive(PartialEq)]
enum Part {
    Ours,
    Base,
    Theirs,
}

fn scan(lines: &[&str]) -> Vec<Found> {
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(line) = lines.get(index) {
        let Some(ours_label) = marker_rest(line, '<') else {
            index += 1;
            continue;
        };
        let start = index;
        let mut part = Part::Ours;
        let (mut ours, mut base, mut theirs) = (String::new(), None::<String>, String::new());
        let mut cursor = index + 1;
        let mut theirs_label = None;
        while let Some(inner) = lines.get(cursor) {
            if marker_rest(inner, '<').is_some() {
                break;
            }
            if part == Part::Ours && marker_rest(inner, '|').is_some() {
                part = Part::Base;
                base = Some(String::new());
            } else if part != Part::Theirs && marker_rest(inner, '=').is_some_and(str::is_empty) {
                part = Part::Theirs;
            } else if part == Part::Theirs && marker_rest(inner, '>').is_some() {
                theirs_label = marker_rest(inner, '>').map(str::to_string);
                break;
            } else {
                match part {
                    Part::Ours => ours.push_str(inner),
                    Part::Base => base.get_or_insert_with(String::new).push_str(inner),
                    Part::Theirs => theirs.push_str(inner),
                }
            }
            cursor += 1;
        }
        match theirs_label {
            Some(closing_label) => {
                found.push(Found {
                    lines: start..cursor + 1,
                    block: ConflictBlock {
                        start_line: u32::try_from(start + 1).unwrap_or(u32::MAX),
                        ours_label: ours_label.to_string(),
                        theirs_label: closing_label,
                        ours,
                        base,
                        theirs,
                    },
                });
                index = cursor + 1;
            }
            None => index = start + 1,
        }
    }
    found
}

pub fn parse_blocks(text: &str) -> Vec<ConflictBlock> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    scan(&lines).into_iter().map(|found| found.block).collect()
}

/// One line of a file with conflict markers as a reader sees it: the markers
/// are gone and each side of a block is its own run of lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewLine {
    /// The 1-based line number in the file and its text.
    Shared(u32, String),
    Ours(String),
    Theirs(String),
}

fn without_newline(line: &str) -> String {
    line.trim_end_matches(['\n', '\r']).to_string()
}

pub fn preview_lines(text: &str) -> Vec<PreviewLine> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut shown = Vec::new();
    let mut cursor = 0;
    let shared = |from: usize, to: usize, out: &mut Vec<PreviewLine>| {
        for (offset, line) in lines.get(from..to).unwrap_or_default().iter().enumerate() {
            let number = u32::try_from(from + offset + 1).unwrap_or(u32::MAX);
            out.push(PreviewLine::Shared(number, without_newline(line)));
        }
    };
    for found in scan(&lines) {
        shared(cursor, found.lines.start, &mut shown);
        shown.extend(
            found
                .block
                .ours
                .lines()
                .map(|line| PreviewLine::Ours(line.to_string())),
        );
        shown.extend(
            found
                .block
                .theirs
                .lines()
                .map(|line| PreviewLine::Theirs(line.to_string())),
        );
        cursor = found.lines.end;
    }
    shared(cursor, lines.len(), &mut shown);
    shown
}

/// The text with block number `only` (or every block when `None`) replaced by the choice.
/// `None` when there is no such block.
pub fn resolve_blocks(text: &str, only: Option<usize>, choice: ConflictChoice) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let found = scan(&lines);
    if found.is_empty() || only.is_some_and(|number| number >= found.len()) {
        return None;
    }
    let mut result = String::new();
    let mut cursor = 0;
    for (number, block) in found.iter().enumerate() {
        if only.is_some_and(|wanted| wanted != number) {
            continue;
        }
        for line in lines.get(cursor..block.lines.start).unwrap_or_default() {
            result.push_str(line);
        }
        match choice {
            ConflictChoice::Ours => result.push_str(&block.block.ours),
            ConflictChoice::Theirs => result.push_str(&block.block.theirs),
            ConflictChoice::Both => {
                result.push_str(&block.block.ours);
                result.push_str(&block.block.theirs);
            }
        }
        cursor = block.lines.end;
    }
    for line in lines.get(cursor..).unwrap_or_default() {
        result.push_str(line);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = "head\n<<<<<<< HEAD\nmine 1\n=======\ntheirs 1\n>>>>>>> feature\nmiddle\n<<<<<<< HEAD\nmine 2\n=======\ntheirs 2\n>>>>>>> feature\ntail\n";

    #[test]
    fn the_preview_drops_the_markers_and_numbers_the_shared_lines() {
        let shown = preview_lines(TWO);
        assert_eq!(
            shown.first(),
            Some(&PreviewLine::Shared(1, "head".to_string()))
        );
        assert_eq!(shown.get(1), Some(&PreviewLine::Ours("mine 1".to_string())));
        assert_eq!(
            shown.get(2),
            Some(&PreviewLine::Theirs("theirs 1".to_string()))
        );
        assert_eq!(
            shown.get(3),
            Some(&PreviewLine::Shared(7, "middle".to_string()))
        );
        assert_eq!(shown.len(), 7);
    }

    #[test]
    fn finds_both_blocks_with_labels_and_lines() {
        let blocks = parse_blocks(TWO);
        assert_eq!(blocks.len(), 2);
        let first = blocks.first().unwrap();
        assert_eq!(first.start_line, 2);
        assert_eq!(first.ours_label, "HEAD");
        assert_eq!(first.theirs_label, "feature");
        assert_eq!(first.ours, "mine 1\n");
        assert_eq!(first.theirs, "theirs 1\n");
        assert_eq!(first.base, None);
        assert_eq!(blocks[1].start_line, 8);
    }

    #[test]
    fn reads_the_base_of_a_diff3_block() {
        let text = "<<<<<<< HEAD\nours\n||||||| merged common ancestors\nbase\n=======\ntheirs\n>>>>>>> topic\n";
        let block = parse_blocks(text).remove(0);
        assert_eq!(block.ours, "ours\n");
        assert_eq!(block.base.as_deref(), Some("base\n"));
        assert_eq!(block.theirs, "theirs\n");
    }

    #[test]
    fn resolves_one_block_and_leaves_the_other() {
        let resolved = resolve_blocks(TWO, Some(1), ConflictChoice::Theirs).unwrap();
        assert!(resolved.contains("theirs 2\ntail"));
        assert!(resolved.contains("<<<<<<< HEAD\nmine 1"));
        assert!(!resolved.contains("mine 2"));
    }

    #[test]
    fn both_keeps_ours_then_theirs() {
        let resolved = resolve_blocks(TWO, None, ConflictChoice::Both).unwrap();
        assert_eq!(
            resolved,
            "head\nmine 1\ntheirs 1\nmiddle\nmine 2\ntheirs 2\ntail\n"
        );
    }

    #[test]
    fn keeps_windows_line_endings() {
        let text = "a\r\n<<<<<<< HEAD\r\nmine\r\n=======\r\ntheirs\r\n>>>>>>> x\r\nz\r\n";
        let resolved = resolve_blocks(text, None, ConflictChoice::Ours).unwrap();
        assert_eq!(resolved, "a\r\nmine\r\nz\r\n");
    }

    #[test]
    fn an_unfinished_block_is_plain_text() {
        let text = "<<<<<<< HEAD\nours\n=======\nno end\n";
        assert!(parse_blocks(text).is_empty());
        assert_eq!(resolve_blocks(text, None, ConflictChoice::Ours), None);
    }

    #[test]
    fn a_line_of_equals_signs_alone_is_not_a_marker() {
        let text = "title\n==========\nbody\n";
        assert!(parse_blocks(text).is_empty());
    }

    #[test]
    fn a_missing_block_number_is_refused() {
        assert_eq!(resolve_blocks(TWO, Some(2), ConflictChoice::Ours), None);
    }
}
