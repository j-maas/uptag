use regex::Regex;

/// A version format detecting and comparing versions.
///
/// The extractor is built on a regular expression that extracts the numbers
/// to be used for the version. The goal is to have all relevant numbers captured
/// in [unnamed capture groups]. The [Regex] syntax is used.
///
/// Note that only unnamed capture groups will be extracted. Named capture groups have no effect.
/// You are responsible for ensuring that all capture groups only capture strings
/// that can be parsed into an unsigned integer. Otherwise, [`extract_from()`] will return `None`.
///
/// This also means that it is not possible to affect the ordering of the extracted numbers.
/// They will always be compared from left to right in the order of the capture groups. As an example,
/// it is not possible to extract a `<minor>.<major>` scheme, where you want to sort first by `<major>` and
/// then by `<minor>`. It will have to be sorted first by `<minor>` then by `<major>`, since `<minor>` is
/// before `<major>`.
///
/// # Examples
///
/// Detect only proper SemVer, without any prefix or suffix:
///
/// ```rust
/// # extern crate updock; use updock::VersionExtractor;
/// # fn main() {
/// let format = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)$").unwrap();
/// assert!(format.matches("1.2.3"));
/// assert!(!format.matches("1.2.3-debian"));
/// # }
/// ```
///
/// Detect a sequential version after a prefix:
///
/// ```rust
/// # extern crate updock; use updock::VersionExtractor;
/// # fn main() {
/// let format = VersionExtractor::parse(r"^debian-r(\d+)$").unwrap();
/// assert!(format.matches("debian-r24"));
/// assert!(!format.matches("debian-r24-alpha"));
/// # }
/// ```
///
/// [unnamed capture groups]: https://docs.rs/regex/1.3.1/regex/#grouping-and-flags
/// [Regex]: https://docs.rs/regex/1.3.1/regex/index.html#syntax
/// [`extract_from()`]: #method.extract_from
#[derive(Debug)]
pub struct VersionExtractor {
    regex: Regex,
}

impl VersionExtractor {
    pub fn parse(regex: &str) -> Result<VersionExtractor, regex::Error> {
        Ok(VersionExtractor {
            regex: Regex::new(regex)?,
        })
    }

    pub fn from(regex: Regex) -> VersionExtractor {
        VersionExtractor { regex }
    }

    pub fn matches(&self, candidate: &str) -> bool {
        self.regex.is_match(candidate)
    }

    pub fn extract_from(&self, candidate: &str) -> Result<Option<Version>, ExtractionError> {
        self.regex
            .captures_iter(candidate)
            .flat_map(|capture| {
                capture
                    .iter()
                    .skip(1) // The first group is the entire match.
                    .filter_map(|maybe_submatch| {
                        maybe_submatch.map(|submatch| {
                            submatch
                                .as_str()
                                .parse()
                                .map_err(|_| ExtractionError::InvalidGroup)
                        })
                    })
                    .collect::<Vec<Result<VersionPart, ExtractionError>>>()
            })
            .collect::<Result<Vec<VersionPart>, ExtractionError>>()
            .map(Version::new)
    }

    pub fn filter<S: AsRef<str>>(&self, candidates: impl IntoIterator<Item = S>) -> Vec<S> {
        candidates
            .into_iter()
            .filter(|candidate| self.matches(candidate.as_ref()))
            .collect()
    }
}

// TODO: Test these errors..as_ref()
#[derive(Debug, PartialEq)]
pub enum ExtractionError {
    InvalidGroup,
    EmptyVersion,
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
pub struct Version {
    parts: Vec<VersionPart>,
}

type VersionPart = u64;

impl Version {
    pub fn new(parts: Vec<VersionPart>) -> Option<Version> {
        if parts.is_empty() {
            None
        } else {
            Some(Version { parts })
        }
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
            let format = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)$").unwrap();
            prop_assert_matches!(format, &valid);
        }

        #[test]
        fn rejects_simple_semver_with_prefix(invalid in r"\PC*[^[:digit:]][[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+\PC*") {
            let format = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)$").unwrap();
            prop_assert_no_match!(format, &invalid);
        }

        #[test]
        fn rejects_simple_semver_with_suffix(invalid in r"\PC*[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+[^[:digit:]]\PC*") {
            let format = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)$").unwrap();
            prop_assert_no_match!(format, &invalid);
        }

        #[test]
        fn extracts_semver(major: u64, minor: u64, patch: u64, suffix in r"[^\d]\PC*") {
            let format = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)+").unwrap();
            let candidate = format!("{}.{}.{}{}", major, minor, patch, suffix);
            let version = Version { parts: vec![major, minor, patch]};
            prop_assert_eq!(format.extract_from(&candidate), Ok(Some(version)));
        }

        #[test]
        fn retains_all_matching_semver_tags(tags in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+")) {
            let format = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)+").unwrap();
            let filtered: Vec<String> = format.filter(&tags).into_iter().cloned().collect();
            prop_assert_eq!(filtered, tags);
        }
    }
}
