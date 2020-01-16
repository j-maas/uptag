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
}

pub type FetchEntry<E> = Result<Tag, E>;

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
        DockerHubTagFetcher { search_limit: 100 }
    }

    pub fn with_search_limit(search_limit: usize) -> Self {
        DockerHubTagFetcher { search_limit }
    }
}

impl TagFetcher for DockerHubTagFetcher {
    type TagIter = std::iter::Take<DockerHubTagIterator>;
    type FetchError = reqwest::Error;

    fn fetch(&self, name: &ImageName) -> Self::TagIter {
        DockerHubTagIterator::new(name).take(self.search_limit)
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
                "https://hub.docker.com/v2/repositories/{}/tags/?page_size={}&page={}&ordering=last_updated",
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

    use crate::image::ImageName;

    #[derive(Debug, PartialEq, Default)]
    pub struct ArrayFetcher {
        content: HashMap<ImageName, Vec<Tag>>,
    }

    impl ArrayFetcher {
        pub fn new() -> Self {
            ArrayFetcher {
                content: HashMap::new(),
            }
        }

        pub fn with(image_name: ImageName, tags: Vec<Tag>) -> ArrayFetcher {
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
    }

    #[derive(Error, Debug, PartialEq)]
    #[error("Failed to fetch tags for image {image_name}.")]
    pub struct FetchError {
        image_name: String,
    }

    impl FetchError {
        pub fn new(image_name: String) -> Self {
            FetchError { image_name }
        }
    }
}
