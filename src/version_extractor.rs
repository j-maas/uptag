use std::fmt;

use regex::Regex;

use pattern_parser::Pattern;

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

impl fmt::Display for VersionExtractor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.pattern)
    }
}

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
        let regex = pattern.regex();
        VersionExtractor { pattern, regex }
    }

    pub fn parse<'a, S>(pattern: S) -> Result<VersionExtractor, Error>
    where
        S: 'a + AsRef<str>,
    {
        Ok(VersionExtractor::new(Pattern::parse(pattern.as_ref())?))
    }

    pub fn breaking_degree(&self) -> usize {
        self.pattern.breaking_degree()
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
        let tag = candidate.tag().as_ref();
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

    pub fn extract_iter<'a, T>(
        &'a self,
        candidates: impl IntoIterator<Item = T> + 'a,
    ) -> impl Iterator<Item = (Version, T)> + 'a
    where
        T: Tagged,
    {
        candidates.into_iter().filter_map(move |candidate| {
            self.extract_from(candidate.tag())
                .map(|version| (version, candidate))
        })
    }

    pub fn max<T>(&self, candidates: impl IntoIterator<Item = T>) -> Option<(Version, T)>
    where
        T: Tagged,
    {
        self.extract_iter(candidates).max_by(|a, b| a.0.cmp(&b.0))
    }
}

pub type Error = pattern_parser::Error;

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

mod pattern_parser {
    use itertools::Itertools;
    use nom::branch::alt;
    use nom::bytes::complete::{tag, take_while1};
    use nom::combinator::{all_consuming, opt, recognize};
    use nom::error::{ParseError, VerboseError};
    use nom::multi::many0;
    use nom::sequence::tuple;
    use nom::IResult;
    use regex::Regex;
    use thiserror::Error;

    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct Pattern {
        parts: Vec<PatternPart>,
        breaking_degree: usize,
    }

    impl Pattern {
        pub fn parse(i: &str) -> Result<Pattern, Error> {
            pattern(i)
                .map(|(_, pattern)| pattern)
                .map_err(|error| Error::new(i, error))
        }

        pub fn breaking_degree(&self) -> usize {
            self.breaking_degree
        }

        pub fn regex(&self) -> Regex {
            use PatternPart::*;
            let inner_regex = self
                .parts
                .iter()
                .map(|part| match part {
                    Literal(literal) => Self::escape_literal(&literal),
                    VersionPart => r"(\d+)".to_string(),
                })
                .join("");
            let raw_regex = format!("^{}$", inner_regex);

            Regex::new(&raw_regex).unwrap()
        }

        fn escape_literal(literal: &str) -> String {
            literal.replace(".", r"\.")
        }
    }

    #[derive(Debug, PartialEq, Error)]
    #[error("{description}")]
    pub struct Error {
        description: String,
    }

    impl Error {
        pub fn new(input: &str, error: nom::Err<VerboseError<&str>>) -> Self {
            use nom::Err::*;
            let description = match error {
                Incomplete(_) => "Incomplete input".to_string(),
                Failure(error) | Error(error) => nom::error::convert_error(input, error),
            };
            Self { description }
        }
    }

    impl std::fmt::Display for Pattern {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}",
                self.parts
                    .iter()
                    .map(|part| {
                        use PatternPart::*;
                        match part {
                            VersionPart => "<>".to_string(),
                            Literal(literal) => literal.clone(),
                        }
                    })
                    .join("")
            )
        }
    }

    #[derive(Debug, PartialEq, Eq, Clone)]
    pub enum PatternPart {
        VersionPart,
        Literal(String),
    }

    pub fn pattern<'a, E>(i: &'a str) -> IResult<&'a str, Pattern, E>
    where
        E: ParseError<&'a str>,
    {
        let (o, (maybe_first, mut breaking, mut compatible)) = all_consuming(tuple((
            opt(outer_literal),
            breaking_parts,
            compatible_parts,
        )))(i)?;

        let breaking_degree = breaking
            .iter()
            .filter(|part| match part {
                PatternPart::VersionPart => true,
                _ => false,
            })
            .count();
        let mut parts = match maybe_first {
            Some(first) => vec![first],
            None => vec![],
        };
        parts.append(&mut breaking);
        parts.append(&mut compatible);
        Ok((
            o,
            Pattern {
                parts,
                breaking_degree,
            },
        ))
    }

    pub fn breaking_parts<'a, E>(i: &'a str) -> IResult<&'a str, Vec<PatternPart>, E>
    where
        E: ParseError<&'a str>,
    {
        many0(alt((inner_literal, breaking_version_part)))(i)
    }

    pub fn compatible_parts<'a, E>(i: &'a str) -> IResult<&'a str, Vec<PatternPart>, E>
    where
        E: ParseError<&'a str>,
    {
        many0(alt((inner_literal, compatible_version_part)))(i)
    }

    pub fn inner_literal<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
    where
        E: ParseError<&'a str>,
    {
        let (o, literal) = take_while1(is_inner_literal)(i)?;
        Ok((o, PatternPart::Literal(literal.to_string())))
    }

    pub fn outer_literal<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
    where
        E: ParseError<&'a str>,
    {
        let (o, literal) = recognize(tuple((
            take_while1(is_outer_literal),
            take_while1(is_inner_literal),
        )))(i)?;
        Ok((o, PatternPart::Literal(literal.to_string())))
    }

    pub fn is_inner_literal(c: char) -> bool {
        is_outer_literal(c) || c == '.' || c == '-'
    }

    pub fn is_outer_literal(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }

    pub fn breaking_version_part<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
    where
        E: ParseError<&'a str>,
    {
        let (o, _) = tag("<!>")(i)?;
        Ok((o, PatternPart::VersionPart))
    }

    pub fn compatible_version_part<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
    where
        E: ParseError<&'a str>,
    {
        let (o, _) = tag("<>")(i)?;
        Ok((o, PatternPart::VersionPart))
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn parses_literal() {
            assert_eq!(
                Pattern::parse("1.2.3"),
                Ok(Pattern {
                    parts: vec![PatternPart::Literal("1.2.3".to_string())],
                    breaking_degree: 0
                })
            );
        }

        #[test]
        fn parses_version_part() {
            assert_eq!(
                Pattern::parse("<>"),
                Ok(Pattern {
                    parts: vec![PatternPart::VersionPart],
                    breaking_degree: 0
                })
            )
        }

        #[test]
        fn parses_semver() {
            use PatternPart::*;
            assert_eq!(
                Pattern::parse("<!>.<>.<>"),
                Ok(Pattern {
                    parts: vec![
                        VersionPart,
                        Literal(".".to_string()),
                        VersionPart,
                        Literal(".".to_string()),
                        VersionPart
                    ],
                    breaking_degree: 1
                })
            )
        }

        #[test]
        fn rejects_invalid_characters() {
            assert_eq!(
                pattern(r"(\d+)\.(\d+)"),
                Err(nom::Err::Error((
                    r"(\d+)\.(\d+)",
                    nom::error::ErrorKind::Eof
                )))
            );
        }

        #[test]
        fn rejects_invalid_break_indicator() {
            assert_eq!(pattern(r"<>.<!>.<>"), Err(nom::Err::Error(())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Borrow;

    use itertools::Itertools;
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
        static ref STRICT_SEMVER: VersionExtractor = VersionExtractor::parse("<>.<>.<>").unwrap();
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

        #[test]
        fn retains_all_matching_semver_tags(tags in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+")) {
            let extractor = &STRICT_SEMVER;
            let filtered: Vec<String> = extractor.filter(tags.clone()).collect();
            prop_assert_eq!(filtered, tags);
        }

        #[test]
        fn removes_all_non_matching_tags(
            valids in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+"),
            invalids in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+-debian"))
        {
            let tags = valids.clone().into_iter().interleave(invalids.into_iter());
            let extractor = &STRICT_SEMVER;
            let filtered: Vec<String> = extractor.filter(tags).collect();
            prop_assert_eq!(filtered, valids);
        }

        #[test]
        fn extracts_all_matching_semver_tags(versions: Vec<SemVer>) {
            let tags: Vec<String> = versions.iter().map(display_semver).collect();
            let extractor = &STRICT_SEMVER;
            let filtered: Vec<(Version, String)> = tags
                .into_iter()
                .filter_map(|tag| {
                    extractor
                        .extract_from(&tag)
                        .map(|version| (version, tag))
                })
                .collect();
            let expected: Vec<(Version, String)> = versions
                .into_iter()
                .map(
                    |v| (Version::from(v), display_semver(v))
                ).collect();
            prop_assert_eq!(filtered, expected);
        }

        #[test]
        fn extracts_only_matching_semver_tags(
            versions: Vec<SemVer>,
            invalids in vec!(r"[[:digit:]]+\.[[:digit:]]+\.[[:digit:]]+-debian")
        ) {
            let tags: Vec<String> = versions
                .iter()
                .map(display_semver)
                .interleave(invalids.into_iter())
                .collect();
            let extractor = &STRICT_SEMVER;
            let filtered: Vec<(Version, String)> = tags
                .into_iter()
                .filter_map(|tag| {
                    extractor
                        .extract_from(&tag)
                        .map(|version| (version, tag))
                })
                .collect();
            let expected: Vec<(Version, String)> = versions
                .into_iter()
                .map(
                    |v| (Version::from(v), display_semver(v))
                ).collect();
            prop_assert_eq!(filtered, expected);
        }

        #[test]
        fn returns_correct_maximum(versions: Vec<SemVer>) {
            let tags = versions.iter().map(display_semver);
            let extractor = &STRICT_SEMVER;
            let max = extractor.max(tags).map(|(_, tag)| tag);
            let expected_max = versions.into_iter().max().map(display_semver);
            prop_assert_eq!(max, expected_max);
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
