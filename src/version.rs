use itertools::Itertools;
use regex::Regex;

use crate::pattern;
use crate::pattern::Pattern;

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
pub struct Version {
    parts: Vec<VersionPart>,
}

type VersionPart = usize;

impl Version {
    pub fn new(parts: Vec<VersionPart>) -> Option<Version> {
        if parts.is_empty() {
            None
        } else {
            Some(Version { parts })
        }
    }

    pub fn update_type(&self, other: &Self, breaking_degree: usize) -> UpdateType {
        if self.sameness_degree_with(other) >= breaking_degree {
            UpdateType::Compatible
        } else {
            UpdateType::Breaking
        }
    }

    fn sameness_degree_with(&self, other: &Self) -> usize {
        self.parts
            .iter()
            .zip(other.parts.iter())
            .take_while(|(l, r)| l == r)
            .count()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateType {
    Compatible,
    Breaking,
}

pub mod extractor {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct VersionExtractor {
        pattern: Pattern,
        regex: Regex,
    }

    impl PartialEq for VersionExtractor {
        fn eq(&self, other: &Self) -> bool {
            self.pattern == other.pattern
        }
    }

    impl Eq for VersionExtractor {}

    impl std::str::FromStr for VersionExtractor {
        type Err = String;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            Self::parse(s).map_err(|error| error.to_string())
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
        pub fn new(pattern: Pattern) -> VersionExtractor {
            let regex = Self::regex_for_pattern(&pattern);
            VersionExtractor { pattern, regex }
        }

        pub fn regex_for_pattern(pattern: &Pattern) -> Regex {
            use pattern::PatternPart::*;
            let inner_regex = pattern
                .parts()
                .iter()
                .map(|part| match part {
                    Literal(literal) => Self::escape_literal(literal),
                    VersionPart => r"(\d+)".to_string(),
                })
                .join("");
            let raw_regex = format!("^{}$", inner_regex);

            Regex::new(&raw_regex).unwrap()
        }

        fn escape_literal(literal: &str) -> String {
            literal.replace(".", r"\.")
        }

        pub fn parse<'a, S>(pattern: S) -> Result<VersionExtractor, pattern::Error>
        where
            S: 'a + AsRef<str>,
        {
            let extractor = VersionExtractor::new(Pattern::parse(pattern.as_ref())?);
            Ok(extractor)
        }

        pub fn pattern(&self) -> &Pattern {
            &self.pattern
        }

        pub fn matches<T>(&self, candidate: T) -> bool
        where
            T: Tagged,
        {
            self.regex.is_match(candidate.tag().as_ref())
        }

        pub fn extract_from<T>(&self, candidate: T) -> Option<Version>
        where
            T: Tagged,
        {
            let tag = candidate.tag();
            let parts = self
                .regex
                .captures(tag) // Only look at the first match.
                .into_iter()
                .flat_map(|captures| {
                    captures
                        .iter()
                        .skip(1) // We are only interested in the capture groups, so we skip the first submatch, since that contains the entire match.
                        .filter_map(|maybe_submatch| {
                            maybe_submatch
                                .map(|submatch| submatch.as_str().parse::<VersionPart>().unwrap())
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            Version::new(parts)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        use std::borrow::Borrow;

        use lazy_static::lazy_static;
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

        lazy_static! {
            static ref STRICT_SEMVER: VersionExtractor =
                VersionExtractor::parse("<>.<>.<>").unwrap();
        }

        // Extraction

        proptest! {
            #[test]
            fn detects_simple_semver(valid in r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+") {
                prop_assert_matches!(&*STRICT_SEMVER, &valid);
            }

            #[test]
            fn rejects_simple_semver_with_prefix(
                invalid in r"\PC*[^[:digit:]][[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+\PC*"
            ) {
                prop_assert_no_match!(&*STRICT_SEMVER, &invalid);
            }

            #[test]
            fn rejects_simple_semver_with_suffix(
                invalid in r"\PC*[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+[^[:digit:]]\PC*"
            ) {
                prop_assert_no_match!(&*STRICT_SEMVER, &invalid);
            }

            #[test]
            fn extracts_semver(version: SemVer) {
                let extractor = VersionExtractor::parse("<>.<>.<>-debian").unwrap();
                let candidate = format!("{}-debian", display_semver(version));
                let version = Version::from(version);
                prop_assert_eq!(extractor.extract_from(&candidate), Some(version));
            }
        }

        // Comparison

        prop_compose! {
            fn version_seq
                ()
                (version in prop::collection::vec(0usize..100, 1..10))
                (index in 0..version.len(), upgrade in 1usize..100, mut version in Just(version))
                -> (Version, Version)
            {
                let smaller = Version::new(version.clone()).unwrap();
                version[index] += upgrade;
                let greater = Version::new(version).unwrap();
                (smaller, greater)
            }
        }

        prop_compose! {
            fn version_seq_no_break
                (size: usize, break_degree: usize)
                (version in prop::collection::vec(0usize..100, size))
                (index in break_degree..version.len(), upgrade in 1usize..100, mut version in Just(version))
                -> (Version, Version)
            {
                let smaller = Version::new(version.clone()).unwrap();
                version[index] += upgrade;
                let greater = Version::new(version).unwrap();
                (smaller, greater)
            }
        }

        prop_compose! {
            fn version_seq_with_break
                (size: usize, break_degree: usize)
                (version in prop::collection::vec(0usize..100, size))
                (index in 0..break_degree, upgrade in 1usize..100, mut version in Just(version))
                -> (Version, Version)
            {
                let smaller = Version::new(version.clone()).unwrap();
                version[index] += upgrade;
                let greater = Version::new(version).unwrap();
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
            fn detects_comptaible_update((smaller, greater) in version_seq_no_break(5, 2)) {
                prop_assert_eq!(smaller.update_type(&greater, 2), UpdateType::Compatible);
            }

            #[test]
            fn detects_breaking_update((smaller, greater) in version_seq_with_break(5, 2)) {
                prop_assert_eq!(smaller.update_type(&greater, 2), UpdateType::Breaking);
            }
        }
    }
}
