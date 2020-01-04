use std::collections::VecDeque;

use log;
use reqwest;
use serde::Deserialize;
use thiserror::Error;

use crate::image::ImageName;

pub trait TagFetcher {
    type TagIter: IntoIterator<Item = Result<Tag, Self::FetchError>>;
    type FetchError: std::error::Error;

    fn fetch(&self, image: &ImageName) -> Self::TagIter;
    fn fetch_until(
        &self,
        image: &ImageName,
        tag: &str,
    ) -> Result<Vec<Tag>, FetchUntilError<Self::FetchError>> {
        let mut found = false;
        let tags = self
            .fetch(image)
            .into_iter()
            .take_while(|result| {
                found = result
                    .as_ref()
                    .map(|candidate| candidate == tag)
                    .unwrap_or(false);
                !found
            })
            .map(|result| result.map_err(FetchUntilError::FetchError))
            .collect::<Result<_, _>>();
        if found {
            tags
        } else {
            Err(FetchUntilError::UnfoundTag(tag.to_string()))
        }
    }
}

#[derive(Error, Debug)]
pub enum FetchUntilError<E>
where
    E: 'static + std::error::Error,
{
    #[error("{0}")]
    FetchError(#[from] E),
    #[error("Failed to find tag `{0}`")]
    UnfoundTag(Tag),
}

#[derive(Debug, Default)]
pub struct DockerHubTagFetcher {
    max_search_amount: usize,
}

#[derive(Deserialize, Debug)]
struct Response {
    next: String,
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

    fn fetch_until(
        &self,
        image: &ImageName,
        tag: &str,
    ) -> Result<Vec<Tag>, FetchUntilError<Self::FetchError>> {
        let mut found = false;
        let tags = self
            .fetch(image)
            .take_while(|result| {
                found = result
                    .as_ref()
                    .map(|candidate| candidate == tag)
                    .unwrap_or(false);
                found
            })
            .map(|result| result.map_err(FetchUntilError::FetchError))
            .take(self.max_search_amount)
            .collect::<Result<_, _>>();
        if !found {
            Err(FetchUntilError::UnfoundTag(tag.to_string()))
        } else {
            tags
        }
    }
}

const FETCH_AMOUNT: usize = 25;

pub struct DockerHubTagIterator {
    fetched: VecDeque<Tag>,
    image_name: ImageName,
    page: usize,
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
            page: 1,
        }
    }

    fn format_name_for_url(name: &ImageName) -> String {
        match name {
            ImageName::Official { image } => format!("library/{}", image),
            ImageName::User { user, image } => format!("{}/{}", user, image),
        }
    }
}

impl Iterator for DockerHubTagIterator {
    type Item = Result<Tag, reqwest::Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if !self.fetched.is_empty() {
            self.fetched.pop_front().map(Ok)
        } else {
            let name_path = Self::format_name_for_url(&self.image_name);
            let url = format!(
                "https://hub.docker.com/v2/repositories/{}/tags/?page_size={}&page={}",
                name_path, FETCH_AMOUNT, self.page
            );

            self.page += 1;

            log::info!("Fetching tags for {}:\n{}", name_path, url);
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

                    next
                })
                .transpose()
        }
    }
}
