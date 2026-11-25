use std::fmt;
use std::str::FromStr;

const MAIN: &str = "main";

/// Short fixed name of a filing target: rules belong to it, the folder path
/// does not. Never reused for another target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetId(String);

/// The text is not a target name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTargetId(pub String);

impl fmt::Display for InvalidTargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a target name (lowercase letters, digits and `-`)",
            self.0
        )
    }
}

impl std::error::Error for InvalidTargetId {}

impl TargetId {
    /// `None` for an empty text or one with a character outside `a-z`, `0-9`, `-`.
    pub fn new(value: &str) -> Option<Self> {
        let valid = !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        valid.then(|| Self(value.to_owned()))
    }

    /// The target every rule had before targets existed.
    pub fn main() -> Self {
        Self(MAIN.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TargetId {
    type Err = InvalidTargetId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value).ok_or_else(|| InvalidTargetId(value.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lowercase_letters_digits_and_dashes_make_a_name() {
        for valid in ["main", "t2", "a-b-3"] {
            assert_eq!(TargetId::new(valid).unwrap().as_str(), valid);
        }
        for invalid in ["", "Main", "a b", "a_b", "ä", "a/b"] {
            assert_eq!(TargetId::new(invalid), None, "{invalid}");
        }
        assert_eq!(TargetId::main(), "main".parse().unwrap());
        "A".parse::<TargetId>().unwrap_err();
    }
}
