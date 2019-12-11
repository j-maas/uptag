use env_logger;
use std::error::Error;

use updock::ImageName;
use updock::{DockerHubTagFetcher, TagFetcher};

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let raw_name = "osixia/openldap";
    let name = ImageName::new(raw_name).unwrap();
    print!("{:?}", DockerHubTagFetcher::fetch(name)?);
    Ok(())
}
