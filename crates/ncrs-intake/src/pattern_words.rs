//! A filing pattern in words.

use ncrs_i18n::{Language, Msg};

fn block_name(name: &str, lang: Language) -> Option<&'static str> {
    let key = match name {
        "year" => Msg::FilingPiecesFieldYear,
        "month" => Msg::FilingPiecesFieldMonth,
        "day" => Msg::FilingPiecesFieldDay,
        "date" => Msg::FilingPiecesFieldDate,
        "sender" | "folder" => Msg::FilingPiecesFieldSender,
        "recipient" => Msg::FilingPiecesFieldRecipient,
        "kind" | "name" => Msg::FilingPiecesFieldKind,
        "number" => Msg::FilingPiecesFieldNumber,
        _ => return None,
    };
    Some(lang.text(key))
}

/// The folders of the pattern, for example "Firma A / Year / Sender"; a file
/// name of its own follows after a dot.
pub fn pattern_in_words(pattern: &str, lang: Language) -> String {
    let (folders, file_name) = pattern.rsplit_once('/').unwrap_or(("", pattern));
    let mut words = folders
        .split('/')
        .map(|segment| segment_in_words(segment, lang))
        .collect::<Vec<_>>()
        .join(" / ");
    if file_name.is_empty() {
        return words;
    }
    let name = segment_in_words(file_name, lang);
    if words.is_empty() {
        return name;
    }
    words.push_str(" · ");
    words.push_str(lang.text(Msg::FilingPiecesFileNamePrefix));
    words.push_str(&name);
    words
}

/// A segment of a pattern with the blocks replaced by their field names; the
/// run `{year}{month}{day}` reads as the date. No braces are left.
fn segment_in_words(segment: &str, lang: Language) -> String {
    let with_date = segment.replace("{year}{month}{day}", "{date}");
    let mut words = String::new();
    let mut rest = with_date.as_str();
    while let Some((before, from)) = rest.split_once('{') {
        words.push_str(before);
        match from.split_once('}') {
            Some((name, after)) => {
                words.push_str(block_name(name, lang).unwrap_or_default());
                rest = after;
            }
            None => {
                rest = from;
            }
        }
    }
    words.push_str(rest);
    words.replace('}', "")
}

#[cfg(test)]
mod tests {
    use crate::FILING_PLACEHOLDERS;

    use super::*;

    #[test]
    fn the_pattern_reads_in_words_without_braces() {
        for (pattern, lang, words) in [
            (
                "Firma A/{year}/{sender}/",
                Language::German,
                "Firma A / Jahr / Absender",
            ),
            ("{year}/{sender}/", Language::English, "Year / Sender"),
            (
                "{year}/{year}{month}{day}_{sender}_{number}",
                Language::German,
                "Jahr · Dateiname: Datum_Absender_Nummer",
            ),
            (
                "{year}/{date}_Rent_{number}",
                Language::German,
                "Jahr · Dateiname: Datum_Rent_Nummer",
            ),
            ("{date}_{number}", Language::German, "Datum_Nummer"),
            ("/", Language::German, ""),
            ("", Language::German, ""),
        ] {
            assert_eq!(pattern_in_words(pattern, lang), words, "{pattern}");
        }
    }

    #[test]
    fn no_block_leaves_a_brace_in_the_words() {
        for lang in [Language::English, Language::German] {
            for name in FILING_PLACEHOLDERS {
                for pattern in [
                    format!("{{{name}}}/"),
                    format!("{{{name}}}"),
                    format!("x{{{name}}}y/{{{name}}}z"),
                ] {
                    let words = pattern_in_words(&pattern, lang);
                    assert!(!words.contains(['{', '}']), "{pattern}: {words}");
                    assert!(!words.is_empty(), "{pattern}");
                }
            }
            assert!(!pattern_in_words("{nope}/{year", lang).contains(['{', '}']));
        }
    }
}
