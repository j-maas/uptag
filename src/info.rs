use regex::Regex;

use crate::image_name::ImageName;
use crate::version_extractor::{Tagged, VersionExtractor};

#[derive(Debug)]
pub struct Info {
    pub image: ImageName,
    pub tag: String,
    pub extractor: Option<VersionExtractor>,
}

impl Info {
    pub fn extract_from<S>(dockerfile: S) -> Result<Vec<Info>, regex::Error>
    where
        S: AsRef<str>,
    {
        // TODO: Document that regexs can't contain ", not even escaped.
        let info_regex = Regex::new(
            r#"FROM ((?P<user>[[:word:]]+)/)?(?P<image>[[:word:]]+):(?P<tag>[[:word:][:punct:]]+)(\s*#\s*updock\s+pattern: "(?P<regex>[^"]*)")?"#,
        )
        .unwrap();
        info_regex
            .captures_iter(dockerfile.as_ref())
            .map(|captures| {
                let maybe_user = captures.name("user").map(|user| user.as_str().into());
                let image = captures.name("image").unwrap().as_str().into();
                let tag = captures.name("tag").unwrap().as_str().into();
                let extractor = captures
                    .name("regex")
                    .map(|r| VersionExtractor::parse(r.as_str()))
                    .transpose()?;

                Ok(Info {
                    image: ImageName::from(maybe_user, image),
                    tag,
                    extractor,
                })
            })
            .collect()
    }
}

impl Tagged for Info {
    fn tag(&self) -> &str {
        &self.tag
    }
}

#[cfg(test)]
mod test {
    use super::*;

    impl PartialEq for Info {
        fn eq(&self, other: &Self) -> bool {
            self.image == other.image
                && self.tag == other.tag
                && self.extractor.as_ref().map(|e| e.as_str())
                    == other.extractor.as_ref().map(|e| e.as_str())
        }
    }

    impl Eq for Info {}

    #[test]
    fn extracts_full_info() {
        let dockerfile =
            "FROM bitnami/dokuwiki:2.3.12  # updock pattern: \"^(\\d+)\\.(\\d+)\\.(\\d+)$\"";
        assert_eq!(
            Info::extract_from(dockerfile),
            Ok(vec![Info {
                image: ImageName::User {
                    user: "bitnami".into(),
                    image: "dokuwiki".into()
                },
                tag: "2.3.12".into(),
                extractor: Some(VersionExtractor::parse("^(\\d+)\\.(\\d+)\\.(\\d+)$").unwrap())
            }])
        )
    }

    #[test]
    fn extracts_minimal_info() {
        let dockerfile = "FROM ubuntu:14.04";
        assert_eq!(
            Info::extract_from(dockerfile),
            Ok(vec![Info {
                image: ImageName::Official {
                    image: "ubuntu".into()
                },
                tag: "14.04".into(),
                extractor: None
            }])
        )
    }

    #[test]
    fn does_not_match_empty_tag() {
        let dockerfile = "FROM ubuntu";
        assert_eq!(Info::extract_from(dockerfile), Ok(vec![]))
    }

    #[test]
    fn does_not_match_digest() {
        let dockerfile =
            "FROM ubuntu@bcf9d02754f659706860d04fd261207db010db96e782e2eb5d5bbd7168388b89";
        assert_eq!(Info::extract_from(dockerfile), Ok(vec![]))
    }
}
