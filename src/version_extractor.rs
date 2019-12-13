use std::fmt;

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
/// let extractor = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)$").unwrap();
/// assert!(extractor.matches("1.2.3"));
/// assert!(!extractor.matches("1.2.3-debian"));
/// # }
/// ```
///
/// Detect a sequential version after a prefix:
///
/// ```rust
/// # extern crate updock; use updock::VersionExtractor;
/// # fn main() {
/// let extractor = VersionExtractor::parse(r"^debian-r(\d+)$").unwrap();
/// assert!(extractor.matches("debian-r24"));
/// assert!(!extractor.matches("debian-r24-alpha"));
/// # }
/// ```
///
/// [unnamed capture groups]: https://docs.rs/regex/1.3.1/regex/#grouping-and-flags
/// [Regex]: https://docs.rs/regex/1.3.1/regex/index.html#syntax
/// [`extract_from()`]: #method.extract_from
#[derive(Debug, Clone)]
pub struct VersionExtractor {
    regex: Regex,
}

impl PartialEq for VersionExtractor {
    fn eq(&self, other: &Self) -> bool {
        self.regex.as_str() == other.regex.as_str()
    }
}

impl Eq for VersionExtractor {}

impl fmt::Display for VersionExtractor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for VersionExtractor {
    type Err = regex::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

pub trait Tagged {
    fn tag(&self) -> &str;
}

impl<S> Tagged for S
where
    S: AsRef<str>,
{
    fn tag(&self) -> &str {
        self.as_ref()
    }
}

impl VersionExtractor {
    pub fn as_str(&self) -> &str {
        self.regex.as_str()
    }

    pub fn parse<S>(regex: S) -> Result<VersionExtractor, regex::Error>
    where
        S: AsRef<str>,
    {
        Ok(VersionExtractor {
            regex: Regex::new(regex.as_ref())?,
        })
    }

    pub fn from(regex: Regex) -> VersionExtractor {
        VersionExtractor { regex }
    }

    pub fn matches<T>(&self, candidate: T) -> bool
    where
        T: Tagged,
    {
        self.regex.is_match(candidate.tag())
    }

    pub fn extract_from<T>(&self, candidate: T) -> Result<Option<Version>, ExtractionError>
    where
        T: Tagged,
    {
        self.regex
            .captures_iter(candidate.tag())
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

    pub fn filter<'a, T>(
        &'a self,
        candidates: impl IntoIterator<Item = T> + 'a,
    ) -> impl Iterator<Item = T> + 'a
    where
        T: Tagged,
    {
        candidates
            .into_iter()
            .filter(move |candidate| self.matches(candidate.tag()))
    }

    pub fn extract<'a, T>(
        &'a self,
        candidates: impl IntoIterator<Item = T> + 'a,
    ) -> impl Iterator<Item = Result<(Version, T), ExtractionError>> + 'a
    where
        T: Tagged,
    {
        candidates.into_iter().filter_map(move |candidate| {
            self.extract_from(candidate.tag())
                .transpose()
                .map(|result| result.map(|version| (version, candidate)))
        })
    }

    pub fn max<T>(
        &self,
        candidates: impl IntoIterator<Item = T>,
    ) -> Result<Option<T>, ExtractionError>
    where
        T: Tagged,
    {
        let result: Result<Vec<(Version, T)>, ExtractionError> = self.extract(candidates).collect();
        result.map(|versions| {
            versions
                .into_iter()
                .max_by(|a, b| a.0.cmp(&b.0))
                .map(|(_, tag)| tag)
        })
    }
}

// TODO: Test these errors...
#[derive(Debug, PartialEq)]
pub enum ExtractionError {
    InvalidGroup,
    EmptyVersion,
}

impl fmt::Display for ExtractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "An error occurred while extracting a version.")
    }
}

impl std::error::Error for ExtractionError {}

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

    pub fn should_upgrade_to(&self, other: Self, breaking_degree: usize) -> bool {
        self.sameness_degree_with(other) >= breaking_degree
    }

    pub fn sameness_degree_with(&self, other: Self) -> usize {
        self.parts
            .iter()
            .zip(other.parts.iter())
            .take_while(|(l, r)| l == r)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Borrow;

    use itertools::Itertools;
    use proptest::prelude::*;

    type SemVer = (VersionPart, VersionPart, VersionPart);

    fn display_semver<S>(version: S) -> String
    where
        S: Borrow<SemVer>,
    {
        let version = version.borrow();
        format!("{}.{}.{}", version.0, version.1, version.2)
    }

    impl<S> From<S> for Version
    where
        S: Borrow<SemVer>,
    {
        fn from(other: S) -> Self {
            let other = other.borrow();
            Version {
                parts: vec![other.0, other.1, other.2],
            }
        }
    }

    macro_rules! prop_assert_matches {
        ($extractor:expr, $string:expr) => {
            prop_assert!(
                $extractor.matches($string),
                "{:?} did not match '{:?}'.",
                $extractor,
                $string
            );
        };
    }

    macro_rules! prop_assert_no_match {
        ($extractor:expr, $string:expr) => {
            prop_assert!(
                !$extractor.matches($string),
                "{:?} should not match '{}'.",
                $extractor,
                $string
            );
        };
    }

    fn strict_semver_extractor() -> VersionExtractor {
        VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)$").unwrap()
    }

    // Extraction

    proptest! {
        #[test]
        fn detects_simple_semver(valid in r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+") {
            prop_assert_matches!(strict_semver_extractor(), &valid);
        }

        #[test]
        fn rejects_simple_semver_with_prefix(
            invalid in r"\PC*[^[:digit:]][[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+\PC*"
        ) {
            prop_assert_no_match!(strict_semver_extractor(), &invalid);
        }

        #[test]
        fn rejects_simple_semver_with_suffix(
            invalid in r"\PC*[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+[^[:digit:]]\PC*"
        ) {
            prop_assert_no_match!(strict_semver_extractor(), &invalid);
        }

        #[test]
        fn extracts_semver(version: SemVer, suffix in r"[^\d]\PC*") {
            let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)\.(\d+)").unwrap();
            let candidate = format!("{}{}", display_semver(version), suffix);
            let version = Version::from(version);
            prop_assert_eq!(extractor.extract_from(&candidate), Ok(Some(version)));
        }

        #[test]
        fn retains_all_matching_semver_tags(tags in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+")) {
            let extractor = strict_semver_extractor();
            let filtered: Vec<String> = extractor.filter(tags.clone()).collect();
            prop_assert_eq!(filtered, tags);
        }

        #[test]
        fn removes_all_non_matching_tags(
            valids in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+"),
            invalids in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+-debian"))
        {
            let tags = valids.clone().into_iter().interleave(invalids.into_iter());
            let extractor = strict_semver_extractor();
            let filtered: Vec<String> = extractor.filter(tags).collect();
            assert_eq!(filtered, valids);
        }

        #[test]
        fn extracts_all_matching_semver_tags(versions: Vec<SemVer>) {
            let tags: Vec<String> = versions.iter().map(display_semver).collect();
            let extractor = strict_semver_extractor();
            let filtered: Result<Vec<(Version, String)>, _> = extractor.extract(tags).collect();
            let expected = versions.into_iter().map(|v| (Version::from(v), display_semver(v))).collect();
            prop_assert_eq!(filtered, Ok(expected));
        }

        #[test]
        fn extracts_only_matching_semver_tags(
            versions: Vec<SemVer>,
            invalids in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+-debian")
        ) {
            let tags: Vec<String> = versions.iter().map(display_semver).interleave(invalids.into_iter()).collect();
            let extractor = strict_semver_extractor();
            let filtered: Result<Vec<(Version, String)>, _> = extractor.extract(tags).collect();
            let expected = versions.into_iter().map(|v| (Version::from(v), display_semver(v))).collect();
            prop_assert_eq!(filtered, Ok(expected));
        }

        #[test]
        fn returns_correct_maximum(versions: Vec<SemVer>) {
            let tags = versions.iter().map(display_semver);
            let extractor = strict_semver_extractor();
            let expected_max = versions.iter().max().map(display_semver);
            prop_assert_eq!(extractor.max(tags), Ok(expected_max));
        }
    }

    // Comparison

    prop_compose! {
        fn version_seq
            ()
            (version in prop::collection::vec(0u64..100, 1..10))
            (index in 0..version.len(), upgrade in 1u64..100, mut version in Just(version))
            -> (Version, Version)
        {
            let smaller = Version::new(version.clone()).unwrap();
            version[index] += upgrade;
            let greater = Version::new(version.clone()).unwrap();
            (smaller, greater)
        }
    }

    prop_compose! {
        fn version_seq_no_break
            (size: usize, break_degree: usize)
            (version in prop::collection::vec(0u64..100, size))
            (index in break_degree..version.len(), upgrade in 1u64..100, mut version in Just(version))
            -> (Version, Version)
        {
            let smaller = Version::new(version.clone()).unwrap();
            version[index] += upgrade;
            let greater = Version::new(version.clone()).unwrap();
            (smaller, greater)
        }
    }

    prop_compose! {
        fn version_seq_with_break
            (size: usize, break_degree: usize)
            (version in prop::collection::vec(0u64..100, size))
            (index in 0..break_degree, upgrade in 1u64..100, mut version in Just(version))
            -> (Version, Version)
        {
            let smaller = Version::new(version.clone()).unwrap();
            version[index] += upgrade;
            let greater = Version::new(version.clone()).unwrap();
            (smaller, greater)
        }
    }

    proptest! {
        #[test]
        fn detects_greater_version(
            (smaller, greater) in version_seq()
        ) {
            prop_assert!(smaller.lt(&greater))
        }

        #[test]
        fn allows_nonbreaking_upgrade((smaller, greater) in version_seq_no_break(5, 2)) {
            prop_assert!(smaller.should_upgrade_to(greater, 2));
        }

        #[test]
        fn prevents_breaking_upgrade((smaller, greater) in version_seq_with_break(5, 2)) {
            prop_assert!(!smaller.should_upgrade_to(greater, 2));
        }
    }
}
