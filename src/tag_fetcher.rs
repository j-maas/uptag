use std::collections::VecDeque;

use log;
use reqwest;
use serde::Deserialize;

use crate::image::ImageName;

/// Enables fetching of tags belonging to an image.
pub trait TagFetcher {
    type TagIter: IntoIterator<Item = FetchEntry<Self::FetchError>>;
    type FetchError: std::error::Error;

    /// Constructs a fallible iterator over the `image`'s tags ordered
    /// from newest to oldest.
    ///
    /// The order of tags has be antichronological in the sense that updates
    /// have to appear before the tags they update.
    ///
    /// # Errors
    /// If the `TagFetcher` encounters an error, it will emit an error variant
    /// as the next iterator item.
    ///
    /// [`fetch_until`]: #method.fetch_until
    fn fetch(&self, image: &ImageName) -> Self::TagIter;

    /// Fetches all tags until before `until_tag` is encountered.
    ///
    /// The `until_tag` itself is excluded from the resulting list.
    ///
    /// # Errors
    /// Any [`FetchError`] encountered while fetching new tags will be forwarded.
    ///
    /// If `until_tag` could not be found, an [`UnfoundTag`] error variant is emitted.
    ///
    /// [`FetchError`]: #associatedtype.FetchError
    /// [`UnfoundTag`]: enum.FetchUntilError.html#variant.UnfoundTag
    fn fetch_until(
        &self,
        image: &ImageName,
        until_tag: &str,
    ) -> Result<(Vec<Tag>, CurrentTag), Self::FetchError> {
        let tags = self.fetch(image).into_iter();
        Self::until_tag(tags, until_tag)
    }

    fn until_tag(
        tags: impl Iterator<Item = FetchEntry<Self::FetchError>>,
        until_tag: &str,
    ) -> Result<(Vec<Tag>, CurrentTag), Self::FetchError> {
        let mut found = false;
        let tags_result = tags
            .take_while(|result| {
                found = result
                    .as_ref()
                    .map(|candidate| candidate == until_tag)
                    .unwrap_or(false);
                !found
            })
            .collect::<Result<Vec<_>, _>>();

        tags_result.map(|tags| {
            let current_tag = if found {
                CurrentTag::Found
            } else {
                CurrentTag::NotEncountered {
                    searched_amount: tags.len(),
                }
            };
            (tags, current_tag)
        })
    }
}

pub type FetchEntry<E> = Result<Tag, E>;

#[derive(Debug, PartialEq, Eq)]
pub enum CurrentTag {
    Found,
    NotEncountered { searched_amount: usize },
}

#[derive(Debug, Default)]
pub struct DockerHubTagFetcher {
    search_limit: usize,
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

impl DockerHubTagFetcher {
    pub fn new() -> Self {
        DockerHubTagFetcher { search_limit: 500 }
    }

    pub fn with_search_limit(search_limit: usize) -> Self {
        DockerHubTagFetcher { search_limit }
    }
}

impl TagFetcher for DockerHubTagFetcher {
    type TagIter = DockerHubTagIterator;
    type FetchError = reqwest::Error;

    fn fetch(&self, name: &ImageName) -> Self::TagIter {
        DockerHubTagIterator::new(name)
    }

    fn fetch_until(
        &self,
        image: &ImageName,
        until_tag: &str,
    ) -> Result<(Vec<Tag>, CurrentTag), Self::FetchError> {
        let tags = self.fetch(image).take(self.search_limit);
        Self::until_tag(tags, until_tag)
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
            Next(page) => Some(page.clone()),
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

    #[derive(Debug, PartialEq)]
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
    }

    #[derive(Error, Debug, PartialEq)]
    #[error("Failed to fetch tags for image {image_name}.")]
    pub struct FetchError {
        image_name: String,
    }

    mod until_tag {
        use super::*;

        #[test]
        fn stops_before_tag() {
            let tags = vec![
                "14.06".to_string(),
                "14.05".to_string(),
                "14.04".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ]
            .into_iter()
            .map(Ok);

            assert_eq!(
                ArrayFetcher::until_tag(tags, "14.04"),
                Ok((
                    vec!["14.06".to_string(), "14.05".to_string(),],
                    CurrentTag::Found
                ))
            );
        }

        #[test]
        fn signals_missing_tag() {
            let tags = vec![
                "14.06".to_string(),
                "14.05".to_string(),
                "14.03".to_string(),
                "13.03".to_string(),
            ]
            .into_iter()
            .map(Ok);

            assert_eq!(
                ArrayFetcher::until_tag(tags, "14.04"),
                Ok((
                    vec![
                        "14.06".to_string(),
                        "14.05".to_string(),
                        "14.03".to_string(),
                        "13.03".to_string(),
                    ],
                    CurrentTag::NotEncountered { searched_amount: 4 }
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
}
