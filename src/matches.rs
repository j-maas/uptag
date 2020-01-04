use lazy_static::lazy_static;
use regex::Regex;

use crate::image::{Image, ImageName};
use crate::version_extractor;
use crate::version_extractor::{Tagged, VersionExtractor};

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
        r#"(#\s*updock\s+pattern\s*:\s*"(?P<pattern>[^"]*)"(\s*,\s*breaking\s+degree\s*:\s*(?P<breaking_degree>\d+))?\s*\n+)?\s*FROM\s*((?P<user>[[:word:]-]+)/)?(?P<image>[[:word:]-]+):(?P<tag>[[:word:][:punct:]]+)"#
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

    pub fn pattern(&self) -> &Option<regex::Match<'t>> {
        &self.pattern
    }

    pub fn image(&self) -> Image {
        Image {
            name: ImageName::new(
                self.user.map(|m| m.as_str().to_string()),
                self.image.as_str().to_string(),
            ),
            tag: self.tag.as_str().to_string(),
        }
    }

    pub fn extractor(&self) -> Option<Result<VersionExtractor, version_extractor::Error>> {
        self.pattern.map(|m| VersionExtractor::parse(m.as_str()))
    }

    pub fn breaking_degree(&self) -> Option<usize> {
        self.breaking_degree.map(|m| m.as_str().parse().unwrap())
    }
}

impl<'t> Tagged for Matches<'t> {
    fn tag(&self) -> &str {
        self.tag.as_str()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug)]
    struct ExpectedMatches {
        image_name: ImageName,
        image_tag: &'static str,
        extractor: Option<Result<VersionExtractor, version_extractor::Error>>,
        breaking_degree: Option<usize>,
    }

    impl<'t> PartialEq<Matches<'t>> for ExpectedMatches {
        fn eq(&self, other: &Matches) -> bool {
            let other_image = other.image();
            self.image_name == other_image.name
                && self.image_tag == other_image.tag
                && self.extractor == other.extractor()
                && self.breaking_degree == other.breaking_degree()
        }
    }

    impl<'t> PartialEq<ExpectedMatches> for Matches<'t> {
        fn eq(&self, other: &ExpectedMatches) -> bool {
            other == self
        }
    }

    // This workaround is currenctly necessary,
    // because Rust does not correctly recognize that we can compare Option<Matches> to Option<ExpectedMatches>.
    // For details see https://stackoverflow.com/a/49903940/3287963
    macro_rules! assert_eq_option {
        ($actual:expr, $expected:expr) => {
            match ($actual, $expected) {
                (Some(a), Some(b)) => assert_eq!(a, b),
                (None, None) => (),
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
            "# updock pattern: \"^(\\d+)\\.(\\d+)\\.(\\d+)-ce\\.0$\", breaking degree: 1\nFROM gitlab/gitlab-ce:12.3.2-ce.0";
        assert_eq_option!(
            Matches::first(dockerfile),
            Some(ExpectedMatches {
                image_name: ImageName::User {
                    user: "gitlab".into(),
                    image: "gitlab-ce".into()
                },
                image_tag: "12.3.2-ce.0",
                extractor: Some(VersionExtractor::parse("^(\\d+)\\.(\\d+)\\.(\\d+)-ce\\.0$")),
                breaking_degree: Some(1),
            })
        );
    }

    #[test]
    fn extracts_minimal_statement() {
        let dockerfile = "FROM ubuntu:14.04";
        assert_eq_option!(
            Matches::first(dockerfile),
            Some(ExpectedMatches {
                image_name: ImageName::Official {
                    image: "ubuntu".into()
                },
                image_tag: "14.04",
                extractor: None,
                breaking_degree: None,
            })
        )
    }

    #[test]
    fn does_not_match_empty_tag() {
        let dockerfile = "FROM ubuntu";
        assert_eq!(Matches::first(dockerfile), None)
    }

    #[test]
    fn does_not_match_digest() {
        let dockerfile =
            "FROM ubuntu@bcf9d02754f659706860d04fd261207db010db96e782e2eb5d5bbd7168388b89";
        assert_eq!(Matches::first(dockerfile), None)
    }
}
