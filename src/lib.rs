pub mod docker_compose;
pub mod image;
pub mod matches;
pub mod pattern_parser;
pub mod tag_fetcher;
pub mod version_extractor;

use indexmap::IndexMap;
use itertools::{Either, Itertools};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use image::Image;
use matches::Matches;
use tag_fetcher::{DockerHubTagFetcher, FetchUntilError, TagFetcher};
use version_extractor::{Version, VersionExtractor};

pub struct Updock<T>
where
    T: TagFetcher,
{
    fetcher: T,
}

impl Default for Updock<DockerHubTagFetcher> {
    fn default() -> Self {
        Updock::new(DockerHubTagFetcher::new())
    }
}

impl<T> Updock<T>
where
    T: TagFetcher + std::fmt::Debug + 'static,
    T::FetchError: 'static,
{
    pub fn new(fetcher: T) -> Updock<T> {
        Updock { fetcher }
    }

    pub fn check_input<'a>(&'a self, input: &'a str) -> impl DockerfileResults<T> + 'a {
        Matches::iter(input).map(move |matches| {
            let image = matches.image();
            let result = Self::extract_check_info(
                &image.tag,
                &matches.pattern().map(|m| m.as_str()),
                matches.breaking_degree().unwrap_or(0),
            )
            .and_then(|(current_version, pattern_info)| {
                self.find_update(&image, &current_version, &pattern_info)
                    .map_err(CheckError::FailedFetch)
                    .map(|maybe_update| (maybe_update, pattern_info))
            });

            (image, result)
        })
    }

    fn extract_check_info(
        tag: &str,
        pattern: &Option<&str>,
        breaking_degree: usize,
    ) -> Result<(Version, PatternInfo), CheckError<T>> {
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
        let breaking_degree = breaking_degree;
        Ok((
            current_version,
            PatternInfo {
                extractor,
                breaking_degree,
            },
        ))
    }

    pub fn find_update(
        &self,
        image: &Image,
        current_version: &Version,
        pattern: &PatternInfo,
    ) -> Result<Option<Update>, FetchUntilError<T::FetchError>> {
        let tags = self.fetcher.fetch_until(&image.name, &image.tag)?;
        let (compatible, breaking): (Vec<_>, Vec<_>) = tags
            .into_iter()
            .filter_map(|tag| {
                pattern
                    .extractor
                    .extract_from(&tag)
                    .map(|version| (tag, version))
            })
            .filter(|(_, version)| current_version < version)
            .partition(|(_, version)| {
                current_version.is_breaking_update_to(version, pattern.breaking_degree)
            });

        let max_compatible = compatible
            .into_iter()
            .max_by(|left, right| left.1.cmp(&right.1))
            .map(|(tag, _)| tag);
        let max_breaking = breaking
            .into_iter()
            .max_by(|left, right| left.1.cmp(&right.1))
            .map(|(tag, _)| tag);

        Ok(Update::from(max_compatible, max_breaking))
    }
}

#[derive(Debug, Clone)]
pub struct PatternInfo {
    pub extractor: VersionExtractor,
    pub breaking_degree: usize,
}

type Tag = String;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum Update {
    Compatible(Tag),
    Breaking(Tag),
    Both { compatible: Tag, breaking: Tag },
}

impl Update {
    pub fn from(maybe_compatible: Option<Tag>, maybe_breaking: Option<Tag>) -> Option<Update> {
        match (maybe_compatible, maybe_breaking) {
            (Some(compatible), Some(breaking)) => Some(Update::Both {
                compatible,
                breaking,
            }),
            (Some(compatible), None) => Some(Update::Compatible(compatible)),
            (None, Some(breaking)) => Some(Update::Breaking(breaking)),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum CheckError<T>
where
    // The Debug trait is required here, because the Debug derive incorrectly infers trait bounds on `T`.
    // For details, see https://github.com/rust-lang/rust/issues/26925
    // Including this bound is the easiest workaround, since TagFetchers can easily derive Debug.
    T: TagFetcher + std::fmt::Debug,
    T::FetchError: 'static,
{
    #[error("Failed to fetch tags")]
    FailedFetch(#[source] FetchUntilError<T::FetchError>),
    #[error("The current tag `{tag}` does not match the required pattern `{pattern}`")]
    InvalidCurrentTag { tag: Tag, pattern: String },
    #[error("Failed to find version pattern")]
    UnspecifiedPattern,
    #[error("The version pattern `{pattern}` is invalid")]
    InvalidPattern {
        pattern: String,
        #[source]
        source: regex::Error,
    },
}

pub type DockerfileResult<T> = (Image, Result<(Option<Update>, PatternInfo), CheckError<T>>);

// Trait alias
pub trait DockerfileResults<T>: Iterator<Item = DockerfileResult<T>>
where
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
}

impl<A, T> DockerfileResults<T> for A
where
    A: Iterator<Item = DockerfileResult<T>>,
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
}

#[derive(Debug)]
pub struct DockerfileReport<T>
where
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
    pub no_updates: Vec<Image>,
    pub compatible_updates: IndexMap<Image, Tag>,
    pub breaking_updates: IndexMap<Image, Tag>,
    pub failures: IndexMap<Image, CheckError<T>>,
}

impl<T> DockerfileReport<T>
where
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
    pub fn from(results: impl DockerfileResults<T>) -> Self {
        let (successes, failures): (Vec<_>, IndexMap<_, _>) =
            results.partition_map(|(image, result)| match result {
                Ok(info) => Either::Left((image, info)),
                Err(error) => Either::Right((image, error)),
            });

        let mut no_updates = Vec::new();
        let mut compatible_updates = IndexMap::new();
        let mut breaking_updates = IndexMap::new();

        for (image, (maybe_update, _)) in successes {
            match maybe_update {
                None => no_updates.push(image),
                Some(Update::Compatible(tag)) => {
                    compatible_updates.insert(image, tag);
                }
                Some(Update::Breaking(tag)) => {
                    breaking_updates.insert(image, tag);
                }
                Some(Update::Both {
                    compatible,
                    breaking,
                }) => {
                    compatible_updates.insert(image.clone(), compatible);
                    breaking_updates.insert(image, breaking);
                }
            }
        }

        DockerfileReport {
            no_updates,
            compatible_updates,
            breaking_updates,
            failures,
        }
    }

    pub fn display_successes(&self) -> String {
        let breaking_updates = self
            .breaking_updates
            .iter()
            .map(|(image, tag)| format!("{} -!> {}:{}", image, image.name, tag))
            .collect::<Vec<_>>();
        let compatible_updates = self
            .compatible_updates
            .iter()
            .map(|(image, tag)| format!("{} -> {}:{}", image, image.name, tag))
            .collect::<Vec<_>>();
        let no_updates = self
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
            .failures
            .iter()
            .map(|(image, error)| format!("{}: {}", image, display_error(error)))
            .collect::<Vec<_>>();

        format!("{} with failure:\n{}", failures.len(), failures.join("\n"))
    }
}

pub fn display_error(error: &impl std::error::Error) -> String {
    let mut output = error.to_string();

    let mut next = error.source();
    while let Some(source) = next {
        output.push_str(&format!(": {}", source));
        next = source.source();
    }

    output
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::image::ImageName;

    use std::collections::HashMap;

    #[derive(Debug)]
    struct ArrayFetcher {
        content: HashMap<ImageName, Vec<Tag>>,
    }

    impl ArrayFetcher {
        fn with(image_name: ImageName, tags: Vec<Tag>) -> ArrayFetcher {
            let mut content = HashMap::new();
            content.insert(image_name, tags);
            ArrayFetcher { content }
        }
    }

    impl TagFetcher for ArrayFetcher {
        type TagIter = Vec<Result<Tag, Self::FetchError>>;
        type FetchError = FetchError;

        fn fetch(&self, image: &ImageName) -> Self::TagIter {
            self.content
                .get(image)
                .map(|tags| tags.iter().map(|tag| Ok(tag.clone())).collect::<Vec<_>>())
                .unwrap_or_else(|| {
                    vec![Err(FetchError {
                        image_name: image.to_string(),
                    })]
                })
        }
        fn max_search_amount(&self) -> usize {
            100
        }
    }

    #[derive(Error, Debug)]
    #[error("Failed to fetch tags for image {image_name}.")]
    struct FetchError {
        image_name: String,
    }

    #[test]
    fn finds_compatible_update() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();
        let pattern_info = PatternInfo {
            extractor,
            breaking_degree: 1,
        };

        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.05".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let result = updock.find_update(&image, &current_version, &pattern_info);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(actual, Some(Update::Compatible("14.05".to_string())));
    }

    #[test]
    fn finds_breaking_update() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();
        let pattern_info = PatternInfo {
            extractor,
            breaking_degree: 1,
        };

        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "15.02".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let result = updock.find_update(&image, &current_version, &pattern_info);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(actual, Some(Update::Breaking("15.02".to_string())));
    }

    #[test]
    fn finds_compatible_and_breaking_update() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();
        let pattern_info = PatternInfo {
            extractor,
            breaking_degree: 1,
        };

        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "15.02".to_string(),
                "14.05".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let result = updock.find_update(&image, &current_version, &pattern_info);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(
            actual,
            Some(Update::Both {
                compatible: "14.05".to_string(),
                breaking: "15.02".to_string()
            })
        );
    }

    #[test]
    fn ignores_lesser_version() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();
        let pattern_info = PatternInfo {
            extractor,
            breaking_degree: 1,
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

        let result = updock.find_update(&image, &current_version, &pattern_info);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(actual, None);
    }

    #[test]
    fn finds_compatible_update_from_string() {
        let input = "# updock pattern: \"(\\d+)\\.(\\d+)\", breaking degree: 1\nFROM ubuntu:14.04";

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

        let results = updock.check_input(input);
        let actual_updates = results
            .filter_map(|(image_name, result)| {
                result
                    .ok()
                    .map(|(maybe_update, _)| (image_name, maybe_update))
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual_updates,
            vec![(image, Some(Update::Compatible("14.12".to_string())))]
        );
    }

    #[test]
    fn finds_breaking_update_from_string() {
        let input = "# updock pattern: \"(\\d+)\\.(\\d+)\", breaking degree: 1\nFROM ubuntu:14.04";

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

        let results = updock.check_input(input);
        let actual_updates = results
            .filter_map(|(image, result)| {
                result.ok().map(|(maybe_update, _)| (image, maybe_update))
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual_updates,
            vec![(image, Some(Update::Breaking("15.01".to_string())))]
        );
    }

    #[test]
    fn finds_compatible_and_breaking_update_from_string() {
        let input = "# updock pattern: \"(\\d+)\\.(\\d+)\", breaking degree: 1\nFROM ubuntu:14.04";

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

        let results = updock.check_input(input);
        let actual_updates = results
            .filter_map(|(image, result)| {
                result.ok().map(|(maybe_update, _)| (image, maybe_update))
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual_updates,
            vec![(
                image,
                Some(Update::Both {
                    compatible: "14.12".to_string(),
                    breaking: "15.01".to_string()
                })
            )]
        );
    }

    #[test]
    fn ignores_lesser_versions_from_string() {
        let input = "# updock pattern: \"(\\d+)\\.(\\d+)\", breaking degree: 1\nFROM ubuntu:14.04";

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

        let results = updock.check_input(input);
        let actual_updates = results
            .filter_map(|(image, result)| {
                result.ok().map(|(maybe_update, _)| (image, maybe_update))
            })
            .collect::<Vec<_>>();

        assert_eq!(actual_updates, vec![(image, None)]);
    }

    type TestDockerfileResults = Vec<(
        Image,
        Result<(Option<Update>, PatternInfo), CheckError<ArrayFetcher>>,
    )>;

    #[test]
    fn generates_dockerfile_report() {
        let success_image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let success_tag = "14.05".to_string();
        let success_update = (
            Some(Update::Compatible(success_tag.clone())),
            PatternInfo {
                extractor: VersionExtractor::parse("").unwrap(),
                breaking_degree: 1,
            },
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
                .compatible_updates
                .into_iter()
                .map(|(image, update)| (image, update))
                .collect::<Vec<_>>(),
            vec![(success_image, success_tag)],
        );
        assert_eq!(
            result
                .failures
                .into_iter()
                .map(|(image, _)| image)
                .collect::<Vec<_>>(),
            vec![fail_image]
        );
    }
}
