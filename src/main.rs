use env_logger;
use std::error::Error;

use updock::ImageName;
use updock::VersionExtractor;
use updock::{DockerHubTagFetcher, TagFetcher};

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let raw_name = "osixia/openldap";
    let name = ImageName::new(raw_name).unwrap();
    let tags = DockerHubTagFetcher::fetch(name)?;
    let extractor = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)$")?;
    let newest = extractor.max(tags);
    print!("{:?}", newest);
    Ok(())
}
