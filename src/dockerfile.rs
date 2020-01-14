use itertools::{Either, Itertools};
use thiserror::Error;

use crate::image::Image;
use crate::report::Report;
use crate::tag_fetcher::{CurrentTag, TagFetcher};
use crate::version_extractor;
use crate::version_extractor::VersionExtractor;
use crate::{display_error, Update, Updock, Version};
use matches::Matches;

pub struct Dockerfile {}

impl Dockerfile {
    pub fn check_input<'a, T>(
        updock: &'a Updock<T>,
        input: &'a str,
    ) -> impl DockerfileResults<T::FetchError> + 'a
    where
        T: TagFetcher,
        T::FetchError: 'static,
    {
        Matches::iter(input).map(move |matches| {
            let image = matches.image();
            let result =
                Self::extract_check_info(&image.tag, &matches.pattern().map(|m| m.as_str()))
                    .and_then(|(current_version, pattern_info)| {
                        updock
                            .find_update(&image, &current_version, &pattern_info)
                            .map_err(CheckError::FailedFetch)
                            .map(|(maybe_update, current_tag)| {
                                (maybe_update, current_tag, pattern_info)
                            })
                    });

            (image, result)
        })
    }

    fn extract_check_info<T>(
        tag: &str,
        pattern: &Option<&str>,
    ) -> Result<(Version, VersionExtractor), CheckError<T>>
    where
        T: 'static + std::error::Error,
    {
        use CheckError::*;

        let pattern = pattern.ok_or(UnspecifiedPattern)?;
        let extractor = VersionExtractor::parse(pattern).map_err(|error| InvalidPattern {
            pattern: pattern.to_string(),
            source: error,
        })?;
        let current_version = extractor.extract_from(tag).ok_or(InvalidCurrentTag {
            tag: tag.to_string(),
            pattern: extractor.to_string(),
        })?;
        Ok((current_version, extractor))
    }
}

type Tag = String;

#[derive(Debug, Error, PartialEq)]
pub enum CheckError<T>
where
    T: 'static + std::error::Error,
{
    #[error("Failed to fetch tags")]
    FailedFetch(#[source] T),
    #[error("The current tag `{tag}` does not match the required pattern `{pattern}`")]
    InvalidCurrentTag { tag: Tag, pattern: String },
    #[error("Failed to find version pattern")]
    UnspecifiedPattern,
    #[error("The version pattern `{pattern}` is invalid")]
    InvalidPattern {
        pattern: String,
        #[source]
        source: version_extractor::Error,
    },
}

pub type DockerfileResult<T> = (
    Image,
    Result<(Option<Update>, CurrentTag, VersionExtractor), CheckError<T>>,
);

// Trait alias
pub trait DockerfileResults<T>: Iterator<Item = DockerfileResult<T>>
where
    T: 'static + std::error::Error,
{
}

impl<A, T> DockerfileResults<T> for A
where
    A: Iterator<Item = DockerfileResult<T>>,
    T: 'static + std::error::Error,
{
}

#[derive(Debug)]
pub struct DockerfileReport<T>
where
    T: 'static + std::error::Error,
{
    pub report: Report<Image, (Image, Tag), (Image, ReportError<T>)>,
}

#[derive(Debug, Error, PartialEq)]
pub enum ReportError<T>
where
    T: 'static + std::error::Error,
{
    #[error(transparent)]
    CheckError(#[from] CheckError<T>),
    #[error(
        "Failed to find the current tag `{current_tag}` in the latest {searched_amount} tags (either the tag is missing, or it might be in older tags beyond the search limit)" 
    )]
    CurrentTagNotEncountered {
        current_tag: Tag,
        searched_amount: usize,
    },
}

impl<T> DockerfileReport<T>
where
    T: 'static + std::error::Error,
{
    pub fn from(results: impl DockerfileResults<T>) -> Self {
        let (successes, mut failures): (Vec<_>, Vec<_>) =
            results.partition_map(|(image, result)| match result {
                Ok(info) => Either::Left((image, info)),
                Err(error) => Either::Right((image, ReportError::CheckError(error))),
            });

        let mut no_updates = Vec::new();
        let mut compatible_updates = Vec::new();
        let mut breaking_updates = Vec::new();

        for (image, (maybe_update, current_tag_found, _)) in successes {
            if let CurrentTag::NotEncountered { searched_amount } = current_tag_found {
                failures.push((
                    image.clone(),
                    ReportError::CurrentTagNotEncountered {
                        current_tag: image.tag.clone(),
                        searched_amount,
                    },
                ))
            }
            match maybe_update {
                None => no_updates.push(image),
                Some(Update::Compatible(tag)) => {
                    compatible_updates.push((image, tag));
                }
                Some(Update::Breaking(tag)) => {
                    breaking_updates.push((image, tag));
                }
                Some(Update::Both {
                    compatible,
                    breaking,
                }) => {
                    compatible_updates.push((image.clone(), compatible));
                    breaking_updates.push((image, breaking));
                }
            }
        }

        DockerfileReport {
            report: Report {
                no_updates,
                compatible_updates,
                breaking_updates,
                failures,
            },
        }
    }

    pub fn display_successes(&self) -> String {
        let breaking_updates = self
            .report
            .breaking_updates
            .iter()
            .map(|(image, tag)| format!("{} -!> {}:{}", image, image.name, tag))
            .collect::<Vec<_>>();
        let compatible_updates = self
            .report
            .compatible_updates
            .iter()
            .map(|(image, tag)| format!("{} -> {}:{}", image, image.name, tag))
            .collect::<Vec<_>>();
        let no_updates = self
            .report
            .no_updates
            .iter()
            .map(|image| image.to_string())
            .collect::<Vec<_>>();

        let mut output = Vec::new();

        if !breaking_updates.is_empty() {
            output.push(format!(
                "{} with breaking update:\n{}",
                breaking_updates.len(),
                breaking_updates.join("\n")
            ));
        }
        if !compatible_updates.is_empty() {
            output.push(format!(
                "{} with compatible update:\n{}",
                compatible_updates.len(),
                compatible_updates.join("\n")
            ));
        }
        if !no_updates.is_empty() {
            output.push(format!(
                "{} with no updates:\n{}",
                no_updates.len(),
                no_updates.join("\n")
            ));
        }

        output.join("\n\n")
    }

    pub fn display_failures(&self) -> String {
        let failures = self
            .report
            .failures
            .iter()
            .map(|(image, error)| format!("{}: {}", image, display_error(error)))
            .collect::<Vec<_>>();

        format!("{} with failure:\n{}", failures.len(), failures.join("\n"))
    }
}

mod matches {
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
    }

    lazy_static! {
        static ref STATEMENT: Regex = Regex::new(
            r#"(#\s*updock\s+--pattern\s+"(?P<pattern>[^"]*)"\s*\n[\s\n]*)?\s*FROM\s*((?P<user>[[:word:]-]+)/)?(?P<image>[[:word:]-]+):(?P<tag>[[:word:][:punct:]]+)"#
        ).unwrap();
    }

    impl<'t> Matches<'t> {
        #[allow(dead_code)]
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

        #[allow(dead_code)]
        pub fn extractor(&self) -> Option<Result<VersionExtractor, version_extractor::Error>> {
            self.pattern.map(|m| VersionExtractor::parse(m.as_str()))
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
        }

        impl<'t> PartialEq<Matches<'t>> for ExpectedMatches {
            fn eq(&self, other: &Matches) -> bool {
                let other_image = other.image();
                self.image_name == other_image.name
                    && self.image_tag == other_image.tag
                    && self.extractor == other.extractor()
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
                "# updock --pattern \"<!>.<>.<>-ce.0\"\nFROM gitlab/gitlab-ce:12.3.2-ce.0";
            assert_eq_option!(
                Matches::first(dockerfile),
                Some(ExpectedMatches {
                    image_name: ImageName::User {
                        user: "gitlab".into(),
                        image: "gitlab-ce".into()
                    },
                    image_tag: "12.3.2-ce.0",
                    extractor: Some(VersionExtractor::parse("<!>.<>.<>-ce.0")),
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
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::image::ImageName;
    use crate::tag_fetcher::test::{ArrayFetcher, FetchError};
    use crate::tag_fetcher::CurrentTag;

    type TestDockerfileResults = Vec<(
        Image,
        Result<(Option<Update>, CurrentTag, VersionExtractor), CheckError<FetchError>>,
    )>;

    #[test]
    fn finds_compatible_update_from_string() {
        let input = "# updock --pattern \"<!>.<>\"\nFROM ubuntu:14.04";

        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.12".to_string(),
                "14.05".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let results = Dockerfile::check_input(&updock, input);
        let actual_updates = results
            .map(|(image_name, result)| {
                result.map(|(maybe_update, _, _)| (image_name, maybe_update))
            })
            .collect::<Result<_, _>>();

        assert_eq!(
            actual_updates,
            Ok(vec![(image, Some(Update::Compatible("14.12".to_string())))])
        );
    }

    #[test]
    fn finds_breaking_update_from_string() {
        let input = "# updock --pattern \"<!>.<>\"\nFROM ubuntu:14.04";

        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "15.01".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let results = Dockerfile::check_input(&updock, input);
        let actual_updates = results
            .map(|(image, result)| result.map(|(maybe_update, _, _)| (image, maybe_update)))
            .collect::<Result<_, _>>();

        assert_eq!(
            actual_updates,
            Ok(vec![(image, Some(Update::Breaking("15.01".to_string())))])
        );
    }

    #[test]
    fn finds_compatible_and_breaking_update_from_string() {
        let input = "# updock --pattern \"<!>.<>\"\nFROM ubuntu:14.04";

        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "15.01".to_string(),
                "14.12".to_string(),
                "14.05".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let results = Dockerfile::check_input(&updock, input);
        let actual_updates = results
            .map(|(image, result)| result.map(|(maybe_update, _, _)| (image, maybe_update)))
            .collect::<Result<_, _>>();

        assert_eq!(
            actual_updates,
            Ok(vec![(
                image,
                Some(Update::Both {
                    compatible: "14.12".to_string(),
                    breaking: "15.01".to_string()
                })
            )])
        );
    }

    #[test]
    fn ignores_lesser_versions_from_string() {
        let input = "# updock --pattern \"<!>.<>\"\nFROM ubuntu:14.04";

        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let results = Dockerfile::check_input(&updock, input);
        let actual_updates = results
            .map(|(image, result)| result.map(|(maybe_update, _, _)| (image, maybe_update)))
            .collect::<Result<_, _>>();

        assert_eq!(actual_updates, Ok(vec![(image, None)]));
    }

    #[test]
    fn generates_dockerfile_report() {
        let success_image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let success_tag = "14.05".to_string();
        let success_update = (
            Some(Update::Compatible(success_tag.clone())),
            CurrentTag::Found,
            VersionExtractor::parse("<!>.<>").unwrap(),
        );

        let fail_image = Image {
            name: ImageName::new(None, "error".to_string()),
            tag: "1".to_string(),
        };
        let fail_error = CheckError::UnspecifiedPattern;

        let input: TestDockerfileResults = vec![
            (success_image.clone(), Ok(success_update)),
            (fail_image.clone(), Err(fail_error)),
        ];

        let result = DockerfileReport::from(input.into_iter());
        assert_eq!(
            result
                .report
                .compatible_updates
                .into_iter()
                .collect::<Vec<_>>(),
            vec![(success_image, success_tag)],
        );
        assert_eq!(
            result
                .report
                .failures
                .into_iter()
                .map(|(image, _)| image)
                .collect::<Vec<_>>(),
            vec![fail_image]
        );
    }

    #[test]
    fn missing_current_tag_with_updates_reported() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let tag = "14.05".to_string();
        let update = (
            Some(Update::Compatible(tag.clone())),
            CurrentTag::NotEncountered {
                searched_amount: 100,
            },
            VersionExtractor::parse("<!>.<>").unwrap(),
        );

        let input: TestDockerfileResults = vec![(image.clone(), Ok(update))];

        let result = DockerfileReport::from(input.into_iter());
        assert_eq!(result.report.compatible_updates, vec![(image.clone(), tag)],);
        assert_eq!(
            result.report.failures,
            vec![(
                image.clone(),
                ReportError::CurrentTagNotEncountered {
                    current_tag: image.tag,
                    searched_amount: 100
                }
            )]
        );
    }
}
