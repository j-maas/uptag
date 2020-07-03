pub mod docker_compose;
pub mod dockerfile;
pub mod image;
pub mod pattern;
pub mod report;
pub mod tag_fetcher;
pub mod version_extractor;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use image::Image;
use tag_fetcher::{DockerHubTagFetcher, TagFetcher};
use version_extractor::{UpdateType, Version, VersionExtractor};

pub struct Uptag<T>
where
    T: TagFetcher,
{
    fetcher: T,
}

impl Default for Uptag<DockerHubTagFetcher> {
    fn default() -> Self {
        Uptag::new(DockerHubTagFetcher::new())
    }
}

impl<T> Uptag<T>
where
    T: TagFetcher,
    T::FetchError: 'static,
{
    pub fn new(fetcher: T) -> Uptag<T> {
        Uptag { fetcher }
    }

    pub fn find_update(
        &self,
        image: &Image,
        // TODO: Extract current version in this function.
        current_version: &Version,
        extractor: &VersionExtractor,
    ) -> Result<Update, FindUpdateError<T::FetchError>> {
        let current_tag = &image.tag;

        let mut breaking_update = None;

        let mut searched_amount = 0;
        for tag_result in self.fetcher.fetch(&image.name) {
            searched_amount += 1;

            let tag_candidate = tag_result?;

            if &tag_candidate == current_tag {
                return Ok(Update {
                    compatible: None,
                    breaking: breaking_update,
                });
            }

            if let Some(version_candidate) = extractor.extract_from(&tag_candidate) {
                if &version_candidate < current_version {
                    continue;
                }

                match version_candidate
                    .update_type(current_version, extractor.pattern().breaking_degree())
                {
                    UpdateType::Breaking => {
                        breaking_update = breaking_update.or(Some(tag_candidate));
                    }
                    UpdateType::Compatible => {
                        return Ok(Update {
                            compatible: Some(tag_candidate),
                            breaking: breaking_update,
                        })
                    }
                }
            }
        }

        Err(FindUpdateError::CurrentTagNotEncountered {
            current_tag: current_tag.clone(),
            searched_amount,
        })
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub struct Update {
    pub compatible: Option<Tag>,
    pub breaking: Option<Tag>,
}

type Tag = String;

#[derive(Debug, Error, PartialEq)]
pub enum FindUpdateError<E>
where
    E: 'static + std::error::Error,
{
    #[error("Failed to fetch tags")]
    FetchError(#[from] E),
    #[error("Failed to find current tag `{current_tag}` in the latest {searched_amount} tags")]
    CurrentTagNotEncountered {
        current_tag: Tag,
        searched_amount: usize,
    },
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
        let extractor = VersionExtractor::parse("<!>.<>").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();

        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.05".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let uptag = Uptag::new(fetcher);

        let result = uptag.find_update(&image, &current_version, &extractor);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(
            actual,
            Update {
                compatible: Some("14.05".to_string()),
                breaking: None,
            },
        );
    }

    #[test]
    fn finds_breaking_update() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse("<!>.<>").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();

        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "15.02".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let uptag = Uptag::new(fetcher);

        let result = uptag.find_update(&image, &current_version, &extractor);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(
            actual,
            Update {
                compatible: None,
                breaking: Some("15.02".to_string()),
            },
        );
    }

    #[test]
    fn finds_compatible_and_breaking_update() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse("<!>.<>").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();

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
        let uptag = Uptag::new(fetcher);

        let result = uptag.find_update(&image, &current_version, &extractor);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(
            actual,
            Update {
                compatible: Some("14.05".to_string()),
                breaking: Some("15.02".to_string()),
            },
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

        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        let uptag = Uptag::new(fetcher);

        let result = uptag.find_update(&image, &current_version, &extractor);
        let actual = result.unwrap_or_else(|error| panic!("{}", error));
        assert_eq!(
            actual,
            Update {
                compatible: None,
                breaking: None,
            },
        );
    }

    #[test]
    fn signals_missing_tag() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse("<!>.<>").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();

        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.03".to_string(),
                "14.02".to_string(),
                "13.03".to_string(),
            ],
        );
        let uptag = Uptag::new(fetcher);

        let result = uptag.find_update(&image, &current_version, &extractor);
        assert_eq!(
            result,
            Err(FindUpdateError::CurrentTagNotEncountered {
                current_tag: image.tag,
                searched_amount: 3
            })
        );
    }

    #[test]
    fn forwards_fetch_failure() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let extractor = VersionExtractor::parse("<!>.<>").unwrap();
        let current_version = extractor.extract_from(&image.tag).unwrap();

        // With an empty ArrayFetcher, all queries will return an error, since the image cannot be found.
        let fetcher = ArrayFetcher::new();
        let uptag = Uptag::new(fetcher);

        let result = uptag.find_update(&image, &current_version, &extractor);
        assert_eq!(
            result,
            Err(FindUpdateError::FetchError(
                tag_fetcher::test::FetchError::new(image.name.to_string())
            ))
        );
    }
}
