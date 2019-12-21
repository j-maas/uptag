pub mod docker_compose;
pub mod image;
pub mod matches;
pub mod tag_fetcher;
pub mod version_extractor;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use image::{Image, ImageName};
use matches::Matches;
use tag_fetcher::{DockerHubTagFetcher, TagFetcher};
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
    T::Error: 'static,
{
    pub fn new(fetcher: T) -> Updock<T> {
        Updock { fetcher }
    }

    pub fn check_input<'a>(
        &'a self,
        input: &'a str,
        amount: usize,
    ) -> impl Iterator<Item = (Image, Result<(Option<Update>, PatternInfo), CheckError<T>>)> + 'a
    {
        Matches::iter(input).map(move |matches| {
            let image = matches.image();
            let result = Self::extract_check_info(
                &image.tag,
                &matches.pattern().map(|m| m.as_str()),
                matches.breaking_degree(),
            )
            .and_then(|(current_version, pattern_info)| {
                self.check_update(&image.name, &current_version, &pattern_info, amount)
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

    pub fn check_update(
        &self,
        image_name: &ImageName,
        current_version: &Version,
        pattern: &PatternInfo,
        amount: usize,
    ) -> Result<Option<Update>, T::Error> {
        let tags = self.fetcher.fetch(image_name, amount)?;
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

#[derive(Debug)]
pub struct PatternInfo {
    pub extractor: VersionExtractor,
    pub breaking_degree: usize,
}

type Tag = String;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    T::Error: 'static,
{
    #[error("Failed to fetch tags")]
    FailedFetch(#[source] T::Error),
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

#[cfg(test)]
mod test {
    use super::*;

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
        type Error = FetchError;

        fn fetch(&self, image: &ImageName, _amount: usize) -> Result<Vec<Tag>, Self::Error> {
            self.content
                .get(image)
                .ok_or(FetchError {
                    image_name: image.to_string(),
                })
                .map(|tags| tags.clone())
        }
    }

    #[derive(Error, Debug)]
    #[error("Failed to fetch tags for image {image_name}.")]
    struct FetchError {
        image_name: String,
    }

    #[test]
    fn finds_compatible_update() {
        let image_name = ImageName::new(None, "ubuntu".to_string());
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = extractor.extract_from("14.04".to_string()).unwrap();
        let pattern_info = PatternInfo {
            extractor,
            breaking_degree: 1,
        };

        let fetcher = ArrayFetcher::with(
            image_name.clone(),
            vec![
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
                "14.05".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let result = updock.check_update(&image_name, &current_version, &pattern_info, 25);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(actual, Some(Update::Compatible("14.05".to_string())));
    }

    #[test]
    fn finds_breaking_update() {
        let image_name = ImageName::new(None, "ubuntu".to_string());
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = extractor.extract_from("14.04".to_string()).unwrap();
        let pattern_info = PatternInfo {
            extractor,
            breaking_degree: 1,
        };

        let fetcher = ArrayFetcher::with(
            image_name.clone(),
            vec![
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
                "15.02".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let result = updock.check_update(&image_name, &current_version, &pattern_info, 25);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(actual, Some(Update::Breaking("15.02".to_string())));
    }

    #[test]
    fn finds_compatible_and_breaking_update() {
        let image_name = ImageName::new(None, "ubuntu".to_string());
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = extractor.extract_from("14.04".to_string()).unwrap();
        let pattern_info = PatternInfo {
            extractor,
            breaking_degree: 1,
        };

        let fetcher = ArrayFetcher::with(
            image_name.clone(),
            vec![
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
                "14.05".to_string(),
                "15.02".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let result = updock.check_update(&image_name, &current_version, &pattern_info, 25);
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
        let image_name = ImageName::new(None, "ubuntu".to_string());
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = extractor.extract_from("14.04".to_string()).unwrap();
        let pattern_info = PatternInfo {
            extractor,
            breaking_degree: 1,
        };

        let fetcher = ArrayFetcher::with(
            image_name.clone(),
            vec![
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let result = updock.check_update(&image_name, &current_version, &pattern_info, 25);
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
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
                "14.05".to_string(),
                "14.12".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let results = updock.check_input(input, 25);
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
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
                "15.01".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let results = updock.check_input(input, 25);
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
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
                "14.05".to_string(),
                "14.12".to_string(),
                "15.01".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let results = updock.check_input(input, 25);
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
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let results = updock.check_input(input, 25);
        let actual_updates = results
            .filter_map(|(image, result)| {
                result.ok().map(|(maybe_update, _)| (image, maybe_update))
            })
            .collect::<Vec<_>>();

        assert_eq!(actual_updates, vec![(image, None)]);
    }
}
