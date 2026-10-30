use regex::{Regex, RegexBuilder};

/// Rule patterns are typed by the user and run over text from outside; the
/// regex crate matches in linear time, and these limits stop a pattern from
/// growing a huge program. A pattern over the limit does not compile.
const REGEX_SIZE_LIMIT: usize = 1 << 20;
const REGEX_DFA_SIZE_LIMIT: usize = 1 << 20;

pub fn compile_bounded_regex(source: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(source)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
        .build()
}
