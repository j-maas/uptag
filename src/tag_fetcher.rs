use chrono::NaiveDateTime;
use log;
use reqwest;
use serde::{de, Deserialize, Deserializer};

use crate::image_name::ImageName;

pub trait TagFetcher {
    type Error;

    fn fetch(image: &ImageName, page: &Page) -> Result<Vec<String>, Self::Error>;
}

pub struct Page {
    pub size: u32,
    pub page: u32,
}

pub struct DockerHubTagFetcher {}

#[derive(Deserialize, Debug)]
struct Response {
    next: String,
    results: Vec<TagInfo>,
}

#[derive(Deserialize, Debug)]
struct TagInfo {
    name: String,
    id: u32,
    #[serde(deserialize_with = "naive_date_time_from_str")]
    last_updated: NaiveDateTime,
}

fn naive_date_time_from_str<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S.%fZ").map_err(de::Error::custom)
}

impl TagFetcher for DockerHubTagFetcher {
    type Error = reqwest::Error;

    fn fetch(name: &ImageName, page: &Page) -> Result<Vec<String>, Self::Error> {
        let name_path = Self::format_name_for_url(name);
        let url = format!(
            "https://hub.docker.com/v2/repositories/{}/tags/?page_size={}&page={}",
            name_path, page.size, page.page
        );

        log::info!("Fetching tags for {}:\n{}", name_path, url);
        let mut response = reqwest::get(&url)?;
        log::debug!("Received response with status `{}`.", response.status());
        log::debug!("Reading JSON body...");
        let response: Response = response.json()?;
        log::info!("Fetch was successful.");

        Ok(response.results.into_iter().map(|tag| tag.name).collect())
    }
}

impl DockerHubTagFetcher {
    fn format_name_for_url(name: &ImageName) -> String {
        match name {
            ImageName::Official { image } => format!("library/{}", image),
            ImageName::User { user, image } => format!("{}/{}", user, image),
        }
    }
}
