use super::*;
use crate::open_vsx::ExtensionId;
use crate::vsix::Brightness;

pub fn fetched(labels: &[&str]) -> Fetched {
    Fetched {
        extension: FetchedExtension {
            id: ExtensionId::parse("example.dark-themes").unwrap(),
            display_name: "Example Themes".to_owned(),
            version: "1.2.3".to_owned(),
            license: Some("MIT".to_owned()),
            vsix: Vec::new().into(),
        },
        themes: labels
            .iter()
            .map(|label| ContributedTheme {
                label: (*label).to_owned(),
                brightness: Some(Brightness::Dark),
                path: format!("themes/{label}.json"),
            })
            .collect(),
    }
}
