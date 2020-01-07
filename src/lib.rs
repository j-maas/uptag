pub mod docker_compose;
pub mod dockerfile;
pub mod image;
pub mod report;
pub mod tag_fetcher;
pub mod version_extractor;

use serde::{Deserialize, Serialize};

use image::Image;
use tag_fetcher::{CurrentTag, DockerHubTagFetcher, TagFetcher};
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
    T: TagFetcher + std::fmt::Debug,
    T::FetchError: 'static,
{
    pub fn new(fetcher: T) -> Updock<T> {
        Updock { fetcher }
    }

    pub fn find_update(
        &self,
        image: &Image,
        current_version: &Version,
        pattern: &PatternInfo,
    ) -> Result<(Option<Update>, CurrentTag), T::FetchError> {
        let (tags, current_tag) = self.fetcher.fetch_until(&image.name, &image.tag)?;
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

        Ok((Update::from(max_compatible, max_breaking), current_tag))
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
    use crate::tag_fetcher::test::ArrayFetcher;

    #[test]
    fn finds_compatible_update() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse("<>.<>").unwrap();
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
        assert_eq!(
            actual,
            (
                Some(Update::Compatible("14.05".to_string())),
                CurrentTag::Found
            )
        );
    }

    #[test]
    fn finds_breaking_update() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse("<>.<>").unwrap();
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
        assert_eq!(
            actual,
            (
                Some(Update::Breaking("15.02".to_string())),
                CurrentTag::Found
            )
        );
    }

    #[test]
    fn finds_compatible_and_breaking_update() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse("<>.<>").unwrap();
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
            (
                Some(Update::Both {
                    compatible: "14.05".to_string(),
                    breaking: "15.02".to_string()
                }),
                CurrentTag::Found
            )
        );
    }

    #[test]
    fn ignores_lesser_version() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse("<>.<>").unwrap();
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
        assert_eq!(actual, (None, CurrentTag::Found));
    }
}
