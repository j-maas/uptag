use std::collections::VecDeque;

use log;
use reqwest;
use serde::Deserialize;

use crate::image::ImageName;

/// Enables fetching of tags belonging to an image.
pub trait TagFetcher {
    type TagIter: IntoIterator<Item = Result<Tag, Self::FetchError>>;
    type FetchError: std::error::Error;

    /// Constructs a fallible iterator over the `image`'s tags ordered
    /// from newest to oldest.
    ///
    /// The [`fetch_until`] method relies on the order being antichronological.
    ///
    /// # Errors
    /// If the `TagFetcher` encounters an error, it will emit an error variant
    /// as the next iterator item.
    ///
    /// [`fetch_until`]: #method.fetch_until
    fn fetch(&self, image: &ImageName) -> Self::TagIter;

    /// Provides the maximal number of tags to search in [`fetch_until`].
    ///
    /// [`fetch_until`]: #method.fetch_until
    fn max_search_amount(&self) -> usize;

    /// Fetches all tags until before the provided `tag` is encountered or
    /// the search limit set by [`max_search_amount`] is reached.
    ///
    /// The `tag` itself is excluded from the resulting list.
    ///
    /// # Errors
    /// Any [`FetchError`] encountered while fetching new tags will be forwarded.
    ///
    /// If `tag` could not be found and the search limit is reached, an [`UnfoundTag`]
    /// error variant is emitted.
    ///
    /// [`FetchError`]: #associatedtype.FetchError
    /// [`UnfoundTag`]: enum.FetchUntilError.html#variant.UnfoundTag
    fn fetch_until(
        &self,
        image: &ImageName,
        tag: &str,
    ) -> Result<(Vec<Tag>, CurrentTag), Self::FetchError> {
        let mut found = false;
        let tags_result = self
            .fetch(image)
            .into_iter()
            .take_while(|result| {
                found = result
                    .as_ref()
                    .map(|candidate| candidate == tag)
                    .unwrap_or(false);
                !found
            })
            .take(self.max_search_amount())
            .collect::<Result<_, _>>();

        tags_result.map(|tags| {
            let current_tag = if found {
                CurrentTag::Found
            } else {
                CurrentTag::NotEncountered
            };
            (tags, current_tag)
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CurrentTag {
    Found,
    NotEncountered,
}

#[derive(Debug, Default)]
pub struct DockerHubTagFetcher {
    max_search_amount: usize,
}

#[derive(Deserialize, Debug)]
struct Response {
    next: Option<String>,
    results: Vec<TagInfo>,
}

#[derive(Debug, Deserialize)]
struct TagInfo {
    name: String,
}

type Tag = String;

impl TagFetcher for DockerHubTagFetcher {
    type TagIter = DockerHubTagIterator;
    type FetchError = reqwest::Error;

    fn fetch(&self, name: &ImageName) -> Self::TagIter {
        DockerHubTagIterator::new(name)
    }

    fn max_search_amount(&self) -> usize {
        self.max_search_amount
    }
}

const FETCH_AMOUNT: usize = 25;

pub struct DockerHubTagIterator {
    fetched: VecDeque<Tag>,
    image_name: ImageName,
    next_page: NextPage,
}

enum NextPage {
    First,
    Next(String),
    End,
}

impl NextPage {
    fn get_url(&self, image: &ImageName) -> Option<String> {
        use NextPage::*;
        match self {
            First => Some(format!(
                "https://hub.docker.com/v2/repositories/{}/tags/?page_size={}&page={}",
                Self::format_name_for_url(&image),
                FETCH_AMOUNT,
                1
            )),
            Next(page) => Some(page.to_string()),
            End => None,
        }
    }

    fn format_name_for_url(name: &ImageName) -> String {
        match name {
            ImageName::Official { image } => format!("library/{}", image),
            ImageName::User { user, image } => format!("{}/{}", user, image),
        }
    }
}

impl DockerHubTagFetcher {
    pub fn new() -> Self {
        DockerHubTagFetcher {
            max_search_amount: 500,
        }
    }

    pub fn with_max_search_amount(max_search_amount: usize) -> Self {
        DockerHubTagFetcher { max_search_amount }
    }
}

impl DockerHubTagIterator {
    fn new(image_name: &ImageName) -> Self {
        DockerHubTagIterator {
            fetched: VecDeque::with_capacity(FETCH_AMOUNT),
            image_name: image_name.clone(),
            next_page: NextPage::First,
        }
    }
}

impl Iterator for DockerHubTagIterator {
    type Item = Result<Tag, reqwest::Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if !self.fetched.is_empty() {
            self.fetched.pop_front().map(Ok)
        } else {
            let maybe_url = self.next_page.get_url(&self.image_name);

            maybe_url.and_then(|url| {
                log::info!("Fetching tags for {}:\n{}", self.image_name, url);
                let response_result = reqwest::get(&url);
                response_result
                    .and_then(|mut response| {
                        log::debug!("Received response with status `{}`.", response.status());
                        log::debug!("Reading JSON body...");
                        response.json::<Response>()
                    })
                    .map(|response| {
                        log::info!("Fetch was successful.");

                        let mut tags = response
                            .results
                            .into_iter()
                            .map(|info| info.name)
                            .collect::<VecDeque<_>>();
                        let next = tags.pop_front();
                        self.fetched = tags;

                        match response.next {
                            Some(next_page) => {
                                self.next_page = NextPage::Next(next_page);
                            }
                            None => {
                                self.next_page = NextPage::End;
                            }
                        }

                        next
                    })
                    .transpose()
            })
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    use std::collections::HashMap;

    use thiserror::Error;

    use crate::image::{Image, ImageName};

    #[derive(Debug)]
    pub struct ArrayFetcher {
        content: HashMap<ImageName, Vec<Tag>>,
        max_search_amount: usize,
    }

    impl ArrayFetcher {
        pub fn with(image_name: ImageName, tags: Vec<Tag>) -> ArrayFetcher {
            let mut content = HashMap::new();
            content.insert(image_name, tags);
            ArrayFetcher {
                content,
                max_search_amount: 100,
            }
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
            self.max_search_amount
        }
    }

    #[derive(Error, Debug, PartialEq)]
    #[error("Failed to fetch tags for image {image_name}.")]
    pub struct FetchError {
        image_name: String,
    }

    #[test]
    fn returns_tags_until_current() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.06".to_string(),
                "14.05".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );

        let result = fetcher.fetch_until(&image.name, &image.tag);
        assert_eq!(
            result,
            Ok((
                vec!["14.06".to_string(), "14.05".to_string(),],
                CurrentTag::Found
            ))
        );
    }

    #[test]
    fn signals_missing_tag() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.06".to_string(),
                "14.05".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );

        let result = fetcher.fetch_until(&image.name, &image.tag);
        assert_eq!(
            result,
            Ok((
                vec![
                    "14.06".to_string(),
                    "14.05".to_string(),
                    "14.03".to_string(),
                    "13.03".to_string(),
                ],
                CurrentTag::NotEncountered
            ))
        );
    }

    #[test]
    fn signals_missing_tag_if_outside_search_limit() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let mut fetcher = ArrayFetcher::with(
            image.name.clone(),
            vec![
                "14.06".to_string(),
                "14.05".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ],
        );
        fetcher.max_search_amount = 2;

        let result = fetcher.fetch_until(&image.name, &image.tag);
        assert_eq!(
            result,
            Ok((
                vec!["14.06".to_string(), "14.05".to_string(),],
                CurrentTag::NotEncountered
            ))
        );
    }

    #[test]
    fn forwards_fetch_failure() {
        let image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let fetcher = ArrayFetcher {
            content: HashMap::new(),
            max_search_amount: 10,
        };

        let result = fetcher.fetch_until(&image.name, &image.tag);
        assert_eq!(
            result,
            Err(FetchError {
                image_name: image.name.to_string()
            })
        );
    }
}
