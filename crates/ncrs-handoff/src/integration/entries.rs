//! The registry entries of the Explorer menu, as plain data, so that what is
//! written can be checked on any system.

/// One value under `HKEY_CURRENT_USER`. An empty `name` is the key's default
/// value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub name: String,
    pub data: String,
}

/// The verb key, and what the Explorer puts in for the clicked item: `%1` is the
/// folder or file, `%V` the folder whose background was clicked.
pub const MENU_PLACES: [(&str, &str); 3] = [
    (r"Software\Classes\Directory\shell\ncrs", "%1"),
    (r"Software\Classes\Directory\Background\shell\ncrs", "%V"),
    (r"Software\Classes\*\shell\ncrs", "%1"),
];

fn entry(key: &str, name: &str, data: &str) -> Entry {
    Entry {
        key: key.to_owned(),
        name: name.to_owned(),
        data: data.to_owned(),
    }
}

/// What makes `label` appear in the three context menus and start `exe` with
/// the path.
pub fn menu_entries(exe: &str, label: &str) -> Vec<Entry> {
    MENU_PLACES
        .iter()
        .flat_map(|(key, placeholder)| {
            [
                entry(key, "", label),
                entry(key, "Icon", &format!("\"{exe}\"")),
                entry(
                    &format!(r"{key}\command"),
                    "",
                    &format!("\"{exe}\" \"{placeholder}\""),
                ),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_gets_a_label_an_icon_and_a_command_that_quotes_the_path() {
        let entries = menu_entries(r"C:\Program Files\ncrs\ncrs.exe", "Open in ncrs");
        assert_eq!(entries.len(), 9);
        assert!(entries.contains(&Entry {
            key: r"Software\Classes\Directory\Background\shell\ncrs\command".to_owned(),
            name: String::new(),
            data: "\"C:\\Program Files\\ncrs\\ncrs.exe\" \"%V\"".to_owned(),
        }));
        assert!(entries.contains(&Entry {
            key: r"Software\Classes\*\shell\ncrs\command".to_owned(),
            name: String::new(),
            data: "\"C:\\Program Files\\ncrs\\ncrs.exe\" \"%1\"".to_owned(),
        }));
        assert_eq!(
            entries
                .iter()
                .filter(|written| written.name.is_empty() && written.data == "Open in ncrs")
                .count(),
            3
        );
    }
}
