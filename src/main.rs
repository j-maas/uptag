use std::error::Error;

use env_logger;
use regex::Regex;
use structopt::StructOpt;

use updock::ImageName;
use updock::VersionExtractor;
use updock::{DockerHubTagFetcher, TagFetcher};

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(short, long)]
    image: ImageName,
    #[structopt(short, long)]
    pattern: Regex,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let opt = Opt::from_args();

    let tags = DockerHubTagFetcher::fetch(opt.image)?;

    let extractor = VersionExtractor::from(opt.pattern);

    match extractor.max(tags)? {
        Some(newest) => println!("{}", newest),
        None => println!("No matching version found."),
    }

    Ok(())
}
