use std::fmt;

use regex::Regex;

use crate::image_name::ImageName;
use crate::version_extractor::{Tagged, VersionExtractor};

#[derive(Debug)]
pub struct FromStatement {
    pub image: ImageName,
    pub tag: String,
    pub extractor: Option<VersionExtractor>,
    pub breaking_degree: usize,
}

impl fmt::Display for FromStatement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pattern = match &self.extractor {
            Some(pattern) => format!(" # updock pattern: \"{}\"", pattern.as_str()),
            None => "".to_string(),
        };
        let breaking_degree = if self.breaking_degree == 0 {
            "".to_string()
        } else {
            format!(", breaking degree: {}", self.breaking_degree)
        };
        write!(
            f,
            "FROM {}:{}{}{}",
            self.image, self.tag, pattern, breaking_degree
        )
    }
}

impl FromStatement {
    pub fn extract_from<S>(dockerfile: S) -> Result<Option<FromStatement>, regex::Error>
    where
        S: AsRef<str>,
    {
        Self::statement_regex()
            .captures(dockerfile.as_ref())
            .map(Self::extract_from_captures)
            .transpose()
    }

    pub fn extract_all<S>(dockerfile: S) -> Result<Vec<FromStatement>, regex::Error>
    where
        S: AsRef<str>,
    {
        Self::statement_regex()
            .captures_iter(dockerfile.as_ref())
            .map(Self::extract_from_captures)
            .collect()
    }

    fn statement_regex() -> Regex {
        // TODO: Document that regexs can't contain ", not even escaped.
        Regex::new(
            r#"FROM ((?P<user>[[:word:]]+)/)?(?P<image>[[:word:]]+):(?P<tag>[[:word:][:punct:]]+)(\s*#\s*updock\s+pattern: "(?P<pattern>[^"]*)"(,\s+breaking\s+degree:\s+(?P<breaking_degree>\d+))?)?"#,
        )
        .unwrap()
    }

    fn extract_from_captures(captures: regex::Captures) -> Result<FromStatement, regex::Error> {
        let maybe_user = captures.name("user").map(|user| user.as_str().into());
        let image = captures.name("image").unwrap().as_str().into();
        let tag = captures.name("tag").unwrap().as_str().into();
        let extractor = captures
            .name("pattern")
            .map(|m| VersionExtractor::parse(m.as_str()))
            .transpose()?;
        let breaking_degree = captures
            .name("breaking_degree")
            .map(|m| m.as_str().parse().unwrap())
            .unwrap_or(0);

        Ok(FromStatement {
            image: ImageName::from(maybe_user, image),
            tag,
            extractor,
            breaking_degree,
        })
    }
}

impl Tagged for FromStatement {
    fn tag(&self) -> &str {
        &self.tag
    }
}

#[cfg(test)]
mod test {
    use super::*;

    impl PartialEq for FromStatement {
        fn eq(&self, other: &Self) -> bool {
            self.image == other.image
                && self.tag == other.tag
                && self.extractor.as_ref().map(|e| e.as_str())
                    == other.extractor.as_ref().map(|e| e.as_str())
        }
    }

    impl Eq for FromStatement {}

    #[test]
    fn extracts_full_statement() {
        let dockerfile =
            "FROM bitnami/dokuwiki:2.3.12  # updock pattern: \"^(\\d+)\\.(\\d+)\\.(\\d+)$\", breaking degree: 1";
        assert_eq!(
            FromStatement::extract_from(dockerfile),
            Ok(Some(FromStatement {
                image: ImageName::User {
                    user: "bitnami".into(),
                    image: "dokuwiki".into()
                },
                tag: "2.3.12".into(),
                extractor: Some(VersionExtractor::parse("^(\\d+)\\.(\\d+)\\.(\\d+)$").unwrap()),
                breaking_degree: 1,
            }))
        )
    }

    #[test]
    fn extracts_minimal_statement() {
        let dockerfile = "FROM ubuntu:14.04";
        assert_eq!(
            FromStatement::extract_from(dockerfile),
            Ok(Some(FromStatement {
                image: ImageName::Official {
                    image: "ubuntu".into()
                },
                tag: "14.04".into(),
                extractor: None,
                breaking_degree: 0,
            }))
        )
    }

    #[test]
    fn does_not_match_empty_tag() {
        let dockerfile = "FROM ubuntu";
        assert_eq!(FromStatement::extract_from(dockerfile), Ok(None))
    }

    #[test]
    fn does_not_match_digest() {
        let dockerfile =
            "FROM ubuntu@bcf9d02754f659706860d04fd261207db010db96e782e2eb5d5bbd7168388b89";
        assert_eq!(FromStatement::extract_from(dockerfile), Ok(None))
    }

    #[test]
    fn extract_from_only_extracts_first_match() {
        let dockerfile = "FROM ubuntu:14.04\nFROM osixia/openldap:1.3.0";
        assert_eq!(
            FromStatement::extract_from(dockerfile),
            Ok(Some(FromStatement {
                image: ImageName::Official {
                    image: "ubuntu".into()
                },
                tag: "14.04".into(),
                extractor: None,
                breaking_degree: 0,
            }))
        )
    }
}
