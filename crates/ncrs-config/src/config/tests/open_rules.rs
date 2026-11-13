use super::*;

fn rules_of(config: &Config) -> Vec<OpenRule> {
    config.open.type_rules()
}

#[test]
fn without_the_key_markdown_opens_internally() {
    let config = load_text("version = 1\n");
    assert_eq!(config.open.by_type, None);
    let rules = rules_of(&config);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].extensions, ["md", "markdown"]);
    assert_eq!(rules[0].with, OpenWith::Internal);
    assert_eq!(
        config.open.open_with_for(Path::new("/a/README.md")),
        Some(OpenWith::Internal)
    );
    assert_eq!(config.open.open_with_for(Path::new("/a/x.pdf")), None);
}

#[test]
fn rules_are_read_in_order_and_the_first_match_wins() {
    let config = load_text(
        "[[open.by_type]]\nextensions = [\"MD\", \".txt\"]\nwith = \"system\"\n\n\
         [[open.by_type]]\nextensions = [\"md\", \"pdf\"]\nwith = \"program\"\nprogram = \"open -a Preview {path}\"\n",
    );
    assert_eq!(
        config.open.open_with_for(Path::new("/a/n.Md")),
        Some(OpenWith::System)
    );
    assert_eq!(
        config.open.open_with_for(Path::new("/a/n.txt")),
        Some(OpenWith::System)
    );
    assert_eq!(
        config.open.open_with_for(Path::new("/a/n.PDF")),
        Some(OpenWith::Program(
            ProgramLine::from_text("open -a Preview {path}").unwrap()
        ))
    );
    assert_eq!(config.open.open_with_for(Path::new("/a/n.rs")), None);
}

#[test]
fn a_written_empty_list_switches_the_default_off() {
    let config = load_text("[open]\nby_type = []\n");
    assert_eq!(config.open.open_with_for(Path::new("/a/README.md")), None);
}

#[test]
fn an_invalid_rule_is_an_error_at_its_line() {
    let cases: &[(&str, &str)] = &[
        (
            "unknown with",
            "[[open.by_type]]\nextensions = [\"md\"]\nwith = \"nope\"\n",
        ),
        (
            "program without a command",
            "[[open.by_type]]\nextensions = [\"md\"]\nwith = \"program\"\n",
        ),
        (
            "command on another mode",
            "[[open.by_type]]\nextensions = [\"md\"]\nwith = \"system\"\nprogram = \"x\"\n",
        ),
        (
            "no extension",
            "[[open.by_type]]\nextensions = []\nwith = \"internal\"\n",
        ),
        (
            "unknown key",
            "[[open.by_type]]\nextensions = [\"md\"]\nwith = \"internal\"\nfoo = 1\n",
        ),
    ];
    for (name, text) in cases {
        let (line, reason) = error_line(text);
        assert!(line.is_some(), "{name}: {reason}");
    }
}

#[test]
fn written_rules_read_back_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let rules = vec![
        OpenRule {
            extensions: vec!["md".into()],
            with: OpenWith::Internal,
        },
        OpenRule {
            extensions: vec!["pdf".into(), "epub".into()],
            with: OpenWith::Program(
                ProgramLine::from_text("open -a \"My Viewer\" {path}").unwrap(),
            ),
        },
        OpenRule {
            extensions: vec!["png".into()],
            with: OpenWith::System,
        },
    ];
    let written = write_setting(&path, open_rules_setting(&rules), None).unwrap();
    assert_eq!(written.open.by_type, Some(rules.clone()));
    assert_eq!(load(&path).unwrap().open.by_type, Some(rules));
    let none = write_setting(&path, open_rules_setting(&[]), None).unwrap();
    assert_eq!(none.open.by_type, Some(Vec::new()));
}
