use std::fmt;

use chrono::NaiveDateTime;
use log;
use reqwest;
use serde::{de, Deserialize, Deserializer};

use crate::image_name::ImageName;

pub struct TagFetcher {}

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

impl TagFetcher {
    pub fn fetch(raw_name: &str) -> Result<Vec<String>, Error> {
        let name = ImageName::new(raw_name).ok_or_else(|| Error::InvalidName(raw_name.into()))?;
        let url = format!(
            "https://hub.docker.com/v2/repositories/{}/tags/?page_size=25",
            name
        );

        log::info!("Fetching {}.", url);
        let mut response = reqwest::get(&url).map_err(Error::Request)?;
        log::debug!("Received response: {:?}", response);
        let response: Response = response.json().map_err(Error::Request)?;

        Ok(response
            .results
            .iter()
            .map(|tag| tag.last_updated.to_string())
            .collect())
    }
}

#[derive(Debug)]
pub enum Error {
    Request(reqwest::Error),
    InvalidName(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            Request(err) => write!(f, "{}", err),
            InvalidName(raw) => write!(f, "'{}' is not a valid name.", raw),
        }
    }
}

impl std::error::Error for Error {}
