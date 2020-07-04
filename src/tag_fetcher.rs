use std::collections::VecDeque;

use serde::Deserialize;
use thiserror::Error;

use crate::image::ImageName;

/// Enables fetching of tags belonging to an image.
pub trait TagFetcher {
    type TagIter: IntoIterator<Item = Result<Tag, Self::FetchError>>;
    type FetchError: std::error::Error;

    /// Constructs a fallible iterator over the `image`'s tags ordered
    /// from newest to oldest.
    ///
    /// The order of tags has to be antichronological in the sense that
    /// tags that are updates to another tag have to appear before
    /// that tag.
    ///
    /// # Errors
    /// If the `TagFetcher` encounters an error, it will emit an error variant
    /// as the next iterator item.
    ///
    /// [`fetch_until`]: #method.fetch_until
    fn fetch(&self, image: &ImageName) -> Self::TagIter;
}

/// Fetches tags from DockerHub.
#[derive(Debug, Default)]
pub struct DockerHubTagFetcher {
    search_limit: usize,
}

// API types from DockerHub

#[derive(Deserialize, Debug)]
struct Response {
    results: Vec<TagInfo>,
    next: Option<String>,
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
    type FetchError = DockerHubTagFetcherError;

    fn fetch(&self, name: &ImageName) -> Self::TagIter {
        DockerHubTagIterator::new(name).take(self.search_limit)
    }
}

const FETCH_AMOUNT: usize = 25;

pub struct DockerHubTagIterator {
    image_name: ImageName,
    /// The tags of the current page.
    fetched: VecDeque<Tag>,
    current_page: CurrentPage,
}

enum CurrentPage {
    First,
    Next(String),
    End,
}

impl CurrentPage {
    fn get_url(&self, image: &ImageName) -> Option<String> {
        use CurrentPage::*;
        match self {
            First => Some(format!(
                "https://hub.docker.com/v2/repositories/{image}/tags/?page_size={amount}&page={page}&ordering=last_updated",
                image=Self::format_name_for_url(&image),
                amount=FETCH_AMOUNT,
                page=1
            )),
            Next(page) => Some(page.clone()),
            End => None,
        }
    }

    fn format_name_for_url(name: &ImageName) -> String {
        match name {
            ImageName::Official { image } => format!("library/{image}", image = image),
            ImageName::User { user, image } => {
                format!("{user}/{image}", user = user, image = image)
            }
        }
    }
}

impl DockerHubTagIterator {
    fn new(image_name: &ImageName) -> Self {
        DockerHubTagIterator {
            fetched: VecDeque::with_capacity(FETCH_AMOUNT),
            image_name: image_name.clone(),
            current_page: CurrentPage::First,
        }
    }
}

type DockerHubTagIteratorError = reqwest::Error;

impl Iterator for DockerHubTagIterator {
    type Item = Result<Tag, DockerHubTagFetcherError>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.fetched.is_empty() {
            self.fetched.pop_front().map(Ok)
        } else {
            let url = self.current_page.get_url(&self.image_name)?;

            log::info!(
                "Fetching tags for {image}:\n{url}",
                image = self.image_name,
                url = url
            );
            let response_result = reqwest::blocking::get(&url);
            response_result
                .and_then(|response| {
                    log::debug!("Received response with status `{}`.", response.status());
                    log::debug!("Reading JSON body...");
                    response.json::<Response>()
                })
                .map_err(DockerHubTagFetcherError::FetchError)
                .and_then(|response| {
                    log::info!("Fetch was successful.");

                    let mut tags = response
                        .results
                        .into_iter()
                        .map(|info| info.name)
                        .collect::<VecDeque<_>>();

                    // If the image name is invalid, we will get a 200 OK, but
                    // with an empty tag list. For details, see https://github.com/Y0hy0h/uptag/issues/37
                    if let CurrentPage::First = self.current_page {
                        if tags.is_empty() {
                            return Err(DockerHubTagFetcherError::EmptyTags(
                                self.image_name.clone(),
                            ));
                        }
                    }

                    let next = tags.pop_front();
                    self.fetched = tags;

                    match response.next {
                        Some(next_page) => {
                            self.current_page = CurrentPage::Next(next_page);
                        }
                        None => {
                            self.current_page = CurrentPage::End;
                        }
                    }

                    Ok(next)
                })
                .transpose()
        }
    }
}

#[derive(Debug, Error)]
pub enum DockerHubTagFetcherError {
    #[error(transparent)]
    FetchError(#[from] DockerHubTagIteratorError),
    #[error("The tag list was empty (this might indicate that `{0}` is not a valid image name)")]
    EmptyTags(ImageName),
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
