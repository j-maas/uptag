use std::fmt;

use lazy_static::lazy_static;
use regex::Regex;

use crate::image::ImageName;
use crate::version_extractor::{Tagged, VersionExtractor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromStatement<'t> {
    matches: Matches<'t>,
    extractor: Option<VersionExtractor>,
    breaking_degree: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matches<'t> {
    all: regex::Match<'t>,
    user: Option<regex::Match<'t>>,
    image: regex::Match<'t>,
    tag: regex::Match<'t>,
    pattern: Option<regex::Match<'t>>,
    breaking_degree: Option<regex::Match<'t>>,
}

// TODO: Document that regexs can't contain `"`, not even escaped.
lazy_static! {
    static ref STATEMENT: Regex = Regex::new(
        r#"(#\s*updock\s+pattern\s*:\s*"(?P<pattern>[^"]*)"(\s*,\s*breaking\s+degree\s*:\s*(?P<breaking_degree>\d+))?\s*\n+)?\s*FROM\s*((?P<user>[[:word:]]+)/)?(?P<image>[[:word:]]+):(?P<tag>[[:word:][:punct:]]+)"#
    ).unwrap();
}

impl<'t> Matches<'t> {
    pub fn first(dockerfile: &'t str) -> Option<Matches<'t>> {
        STATEMENT.captures(dockerfile).map(Self::from_captures)
    }

    pub fn iter(dockerfile: &'t str) -> impl Iterator<Item = Matches<'t>> {
        STATEMENT.captures_iter(dockerfile).map(Self::from_captures)
    }

    fn from_captures(captures: regex::Captures<'t>) -> Matches<'t> {
        Matches {
            all: captures.get(0).unwrap(),
            user: captures.name("user"),
            image: captures.name("image").unwrap(),
            tag: captures.name("tag").unwrap(),
            pattern: captures.name("pattern"),
            breaking_degree: captures.name("breaking_degree"),
        }
    }
}

impl<'t> fmt::Display for FromStatement<'t> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let breaking_degree = if self.breaking_degree == 0 {
            "".to_string()
        } else {
            format!(", breaking degree: {}", self.breaking_degree)
        };
        let pattern = match &self.extractor {
            Some(pattern) => format!(
                "# updock pattern: \"{}\"{}\n",
                pattern.as_str(),
                breaking_degree
            ),
            None => "".to_string(),
        };
        write!(f, "{}FROM {}:{}", pattern, self.image(), self.tag())
    }
}

impl<'t> FromStatement<'t> {
    pub fn image(&self) -> ImageName {
        ImageName::from(
            self.matches.user.map(|m| m.as_str().to_string()),
            self.matches.image.as_str().to_string(),
        )
    }

    pub fn tag(&self) -> &str {
        self.matches.tag.as_str()
    }

    pub fn extractor(&self) -> &Option<VersionExtractor> {
        &self.extractor
    }

    pub fn breaking_degree(&self) -> usize {
        self.breaking_degree
    }

    pub fn first(dockerfile: &'t str) -> Result<Option<FromStatement<'t>>, regex::Error> {
        Matches::first(dockerfile).map(Self::from).transpose()
    }

    pub fn iter(dockerfile: &'t str) -> Result<Vec<FromStatement<'t>>, regex::Error> {
        Matches::iter(dockerfile).map(Self::from).collect()
    }

    fn from(matches: Matches<'t>) -> Result<FromStatement<'t>, regex::Error> {
        let extractor = matches
            .pattern
            .map(|m| VersionExtractor::parse(m.as_str()))
            .transpose()?;
        let breaking_degree = matches
            .breaking_degree
            .map(|m| m.as_str().parse().unwrap())
            .unwrap_or(0);

        Ok(FromStatement {
            matches,
            extractor,
            breaking_degree,
        })
    }
}

impl<'t> Tagged for FromStatement<'t> {
    fn tag(&self) -> &str {
        &self.tag()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug)]
    struct ExpectedFromStatment {
        image: ImageName,
        tag: &'static str,
        extractor: Option<VersionExtractor>,
        breaking_degree: usize,
    }

    impl<'t> PartialEq<FromStatement<'t>> for ExpectedFromStatment {
        fn eq(&self, other: &FromStatement) -> bool {
            self.image == other.image()
                && self.tag == other.tag()
                && &self.extractor == other.extractor()
                && self.breaking_degree == other.breaking_degree()
        }
    }

    impl<'t> PartialEq<ExpectedFromStatment> for FromStatement<'t> {
        fn eq(&self, other: &ExpectedFromStatment) -> bool {
            other == self
        }
    }

    // This workaround is necessary for now.
    // For details see https://stackoverflow.com/a/49903940/3287963
    macro_rules! assert_eq_result_option {
        ($actual:expr, $expected:expr) => {
            match ($actual, $expected) {
                (Ok(Some(a)), Ok(Some(b))) => assert_eq!(a, b),
                (Ok(None), Ok(None)) => (),
                (Err(a), Err(b)) => assert_eq!(a, b),
                (a, b) => panic!(
                    r#"assertion failed: `(left == right)`
    left: `{:?}`,
   right: `{:?}`"#,
                    &a, &b
                ),
            }
        };
    }

    #[test]
    fn extracts_full_statement() {
        let dockerfile =
            "# updock pattern: \"^(\\d+)\\.(\\d+)\\.(\\d+)$\", breaking degree: 1\nFROM bitnami/dokuwiki:2.3.12";
        assert_eq_result_option!(
            FromStatement::first(dockerfile),
            Ok(Some(ExpectedFromStatment {
                image: ImageName::User {
                    user: "bitnami".into(),
                    image: "dokuwiki".into()
                },
                tag: "2.3.12",
                extractor: Some(VersionExtractor::parse("^(\\d+)\\.(\\d+)\\.(\\d+)$").unwrap()),
                breaking_degree: 1,
            }))
        );
    }

    #[test]
    fn extracts_minimal_statement() {
        let dockerfile = "FROM ubuntu:14.04";
        assert_eq_result_option!(
            FromStatement::first(dockerfile),
            Ok(Some(ExpectedFromStatment {
                image: ImageName::Official {
                    image: "ubuntu".into()
                },
                tag: "14.04",
                extractor: None,
                breaking_degree: 0,
            }))
        )
    }

    #[test]
    fn does_not_match_empty_tag() {
        let dockerfile = "FROM ubuntu";
        assert_eq!(FromStatement::first(dockerfile), Ok(None))
    }

    #[test]
    fn does_not_match_digest() {
        let dockerfile =
            "FROM ubuntu@bcf9d02754f659706860d04fd261207db010db96e782e2eb5d5bbd7168388b89";
        assert_eq!(FromStatement::first(dockerfile), Ok(None))
    }
}
