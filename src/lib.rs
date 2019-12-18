mod image;
mod matches;
mod tag_fetcher;
mod version_extractor;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use image::{Image, ImageName};
pub use matches::Matches;
pub use tag_fetcher::{DockerHubTagFetcher, TagFetcher};
pub use version_extractor::{Version, VersionExtractor};

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
    T: TagFetcher + std::fmt::Debug,
{
    pub fn new(fetcher: T) -> Updock<T> {
        Updock { fetcher }
    }

    pub fn check_update(
        &self,
        image_name: &ImageName,
        current_version: &VersionTag,
        extractor: &VersionExtractor,
        breaking_degree: usize,
        amount: usize,
    ) -> Result<Option<Update>, CheckError<T>> {
        use CheckError::*;
        let current_version =
            extractor
                .extract_from(&current_version.tag)
                .ok_or(InvalidCurrentTag {
                    tag: current_version.tag.to_string(),
                    pattern: extractor.to_string(),
                })?;

        let tags = self.fetcher.fetch(image_name, amount).map_err(FetchError)?;
        let (compatible, breaking): (Vec<_>, Vec<_>) = tags
            .into_iter()
            .filter_map(|tag| extractor.extract_from(&tag).map(|version| (tag, version)))
            .filter(|(_, version)| current_version.lt(version))
            .partition(|(_, version)| {
                current_version.is_breaking_update_to(version, breaking_degree)
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
pub struct VersionTag {
    tag: Tag,
    version: Version,
}

impl VersionTag {
    pub fn from(extractor: &VersionExtractor, tag: Tag) -> Option<Self> {
        extractor
            .extract_from(&tag)
            .map(|version| VersionTag { tag, version })
    }
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
    T: TagFetcher + std::fmt::Debug,
    T::Error: 'static,
{
    #[error("Failed to fetch tags.")]
    FetchError(#[source] T::Error),
    #[error("The current tag `{tag}` does not match the required pattern `{pattern}`.")]
    InvalidCurrentTag { tag: Tag, pattern: String },
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
        let image_name = ImageName::from(None, "ubuntu".to_string());
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = VersionTag::from(&extractor, "14.04".to_string()).unwrap();

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

        let result = updock.check_update(&image_name, &current_version, &extractor, 1, 25);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(actual, Some(Update::Compatible("14.05".to_string())));
    }

    #[test]
    fn finds_breaking_update() {
        let image_name = ImageName::from(None, "ubuntu".to_string());
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = VersionTag::from(&extractor, "14.04".to_string()).unwrap();

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

        let result = updock.check_update(&image_name, &current_version, &extractor, 1, 25);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(actual, Some(Update::Breaking("15.02".to_string())));
    }

    #[test]
    fn finds_compatible_and_breaking_update() {
        let image_name = ImageName::from(None, "ubuntu".to_string());
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = VersionTag::from(&extractor, "14.04".to_string()).unwrap();

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

        let result = updock.check_update(&image_name, &current_version, &extractor, 1, 25);
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
    fn ignores_not_matching_version() {
        let image_name = ImageName::from(None, "ubuntu".to_string());
        let extractor = VersionExtractor::parse(r"(\d+)\.(\d+)").unwrap();
        let current_version = VersionTag::from(&extractor, "14.04".to_string()).unwrap();

        let fetcher = ArrayFetcher::with(
            image_name.clone(),
            vec![
                "13.03".to_string(),
                "14.03".to_string(),
                "14.04".to_string(),
            ],
        );
        let updock = Updock::new(fetcher);

        let result = updock.check_update(&image_name, &current_version, &extractor, 1, 25);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(actual, None);
    }
}
