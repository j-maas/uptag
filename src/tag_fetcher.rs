use log;
use reqwest;
use serde::{Deserialize};

use crate::image::ImageName;

pub trait TagFetcher {
    type Error: std::error::Error;

    fn fetch(&self, image: &ImageName, amount: usize) -> Result<Vec<Tag>, Self::Error>;
}

#[derive(Debug, Default)]
pub struct DockerHubTagFetcher {}

#[derive(Deserialize, Debug)]
struct Response {
    next: String,
    results: Vec<Tag>,
}

type Tag = String;

impl TagFetcher for DockerHubTagFetcher {
    type Error = reqwest::Error;

    fn fetch(&self, name: &ImageName, amount: usize) -> Result<Vec<Tag>, Self::Error> {
        let name_path = Self::format_name_for_url(name);
        let url = format!(
            "https://hub.docker.com/v2/repositories/{}/tags/?page_size={}&page={}",
            name_path, amount, 1
        );

        log::info!("Fetching tags for {}:\n{}", name_path, url);
        let mut response = reqwest::get(&url)?;
        log::debug!("Received response with status `{}`.", response.status());
        log::debug!("Reading JSON body...");
        let response: Response = response.json()?;
        log::info!("Fetch was successful.");

        Ok(response.results)
    }
}

impl DockerHubTagFetcher {
    pub fn new() -> DockerHubTagFetcher {
        DockerHubTagFetcher {}
    }

    fn format_name_for_url(name: &ImageName) -> String {
        match name {
            ImageName::Official { image } => format!("library/{}", image),
            ImageName::User { user, image } => format!("{}/{}", user, image),
        }
    }
}
