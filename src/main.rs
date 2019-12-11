use std::error::Error;

use env_logger;
use structopt::StructOpt;

use updock::ImageName;
use updock::VersionExtractor;
use updock::{DockerHubTagFetcher, TagFetcher};

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(short, long)]
    image: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let opt = Opt::from_args();

    let name = ImageName::new(&opt.image).unwrap();
    let tags = DockerHubTagFetcher::fetch(name)?;

    let extractor = VersionExtractor::parse(r"^(\d+)\.(\d+)\.(\d+)$")?;

    match extractor.max(tags)? {
        Some(newest) => println!("{}", newest),
        None => println!("No matching version found."),
    }

    Ok(())
}
