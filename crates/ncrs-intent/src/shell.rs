const OPERATORS: &[&str] = &["|", ";", "&&", ">", "<", "$(", "`"];
const COMMANDS: &[&str] = &[
    "ls", "cd", "rm", "mv", "cp", "mkdir", "rmdir", "grep", "cat", "less", "open", "git", "touch",
    "chmod", "chown", "sudo", "echo", "tar", "zip", "unzip", "curl", "wget", "ssh", "scp", "rsync",
    "brew", "cargo", "make", "python", "node",
];
const ARGUMENT_STARTS: &[char] = &['-', '/', '.', '~'];

/// Whether the text reads like a shell command. Hermes never runs it; the chat
/// only offers it to the command line.
pub fn looks_like_shell(text: &str) -> bool {
    if OPERATORS.iter().any(|operator| text.contains(operator)) {
        return true;
    }
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    COMMANDS.contains(&first)
        && words
            .next()
            .is_none_or(|second| second.starts_with(ARGUMENT_STARTS))
}
