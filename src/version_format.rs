use regex::Regex;

/// A version format detecting and comparing versions.
///
/// The syntax is based on [Regex], but adds two special escaped characters:
/// - `\m` which stands for a semantic version (se**m**antic).
/// - `\q` which stands for a sequence number (se**q**uence).
///
/// # Examples
///
/// Detect only proper SemVer, without any prefix or suffix:
///
/// ```rust
/// # extern crate updock; use updock::VersionFormat;
/// # fn main() {
/// let format = VersionFormat::new(r"^\m$").unwrap();
/// assert!(format.matches("1.2.3"));
/// assert!(!format.matches("1.2.3-debian"));
/// # }
/// ```
///
/// Detect a sequential version after a prefix:
///
/// ```rust
/// # extern crate updock; use updock::VersionFormat;
/// # fn main() {
/// let format = VersionFormat::new(r"^debian-r\q$").unwrap();
/// assert!(format.matches("debian-r24"));
/// assert!(!format.matches("debian-r24-alpha"));
/// # }
/// ```
///
/// [Regex]: https://docs.rs/regex/1.3.1/regex/index.html#syntax
#[derive(Debug)]
pub struct VersionFormat {
    regex: Regex,
}

#[derive(Debug)]
struct Replacement {
    escape_code: &'static str,
    normalized: &'static str,
}

const SEMANTIC: Replacement = Replacement {
    escape_code: r"\m",
    normalized: r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+",
};

const SEQUENCE: Replacement = Replacement {
    escape_code: r"\q",
    normalized: r"[[:digit:]]+",
};

const REPLACEMENTS: [Replacement; 2] = [SEMANTIC, SEQUENCE];

impl VersionFormat {
    pub fn new<S: Into<String>>(format: S) -> Result<VersionFormat, regex::Error> {
        // TODO: Warn about missing version escape character.
        let mut normalized_format: String = format.into();
        for rep in &REPLACEMENTS {
            let capture = format!("({})", rep.normalized); // Ensure no capture groups are present in string.
            normalized_format = normalized_format.replace(rep.escape_code, &capture);
        }
        let regex = Regex::new(&normalized_format)?;

        Ok(VersionFormat { regex })
    }

    pub fn matches<S: AsRef<str>>(&self, candidate: S) -> bool {
        self.regex.is_match(candidate.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use proptest::prelude::*;

    macro_rules! prop_assert_matches {
        ($format:ident, $string:expr) => {
            prop_assert!(
                $format.matches($string),
                "{:?} did not match '{:?}'.",
                $format,
                $string
            );
        };
    }

    macro_rules! prop_assert_no_match {
        ($format:ident, $string:expr) => {
            prop_assert!(
                !$format.matches($string),
                "{:?} should not match '{}'.",
                $format,
                $string
            );
        };
    }

    proptest! {
        #[test]
        fn detects_simple_semver(valid in r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+") {
            let format = VersionFormat::new(r"^\m$").unwrap();
            prop_assert_matches!(format, &valid);
        }

        #[test]
        fn rejects_simple_semver_with_prefix(invalid in r".*[^[:digit:]][[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+.*") {
            let format = VersionFormat::new(r"^\m$").unwrap();
            prop_assert_no_match!(format, &invalid);
        }

        #[test]
        fn rejects_simple_semver_with_suffix(invalid in r".*[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+[^[:digit:]].*") {
            let format = VersionFormat::new(r"^\m$").unwrap();
            prop_assert_no_match!(format, &invalid);
        }

        #[test]
        fn detects_simple_sequence(valid in r"[[:digit:]]+") {
            let format = VersionFormat::new(r"^\q$").unwrap();
            prop_assert_matches!(format, &valid);
        }

        #[test]
        fn rejects_simple_sequence_with_prefix(invalid in r".*[^[:digit:]][[:digit:]]+.*") {
            let format = VersionFormat::new(r"^\q$").unwrap();
            prop_assert_no_match!(format, &invalid);
        }

        #[test]
        fn rejects_simple_sequence_with_suffix(invalid in r".*[[:digit:]]+[^[:digit:]].*") {
            let format = VersionFormat::new(r"^\q$").unwrap();
            prop_assert_no_match!(format, &invalid);
        }
    }
}
