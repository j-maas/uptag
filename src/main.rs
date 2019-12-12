use std::error::Error;
use std::io;
use std::io::prelude::*;

use env_logger;
use regex::Regex;
use structopt::StructOpt;

use updock::ImageName;
use updock::Info;
use updock::VersionExtractor;
use updock::{DockerHubTagFetcher, Page, TagFetcher};

#[derive(Debug, StructOpt)]
enum Opts {
    Fetch(FetchOpts),
    Match(MatchOpts),
}

#[derive(Debug, StructOpt)]
struct FetchOpts {
    #[structopt(short, long)]
    image: ImageName,
    #[structopt(short, long)]
    pattern: Option<Regex>,
    #[structopt(short, long, default_value = "25")]
    amount: u32,
}

#[derive(Debug, StructOpt)]
struct MatchOpts {
    #[structopt(short, long)]
    pattern: Regex,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let opts = Opts::from_args();

    use Opts::*;
    match opts {
        Fetch(opts) => fetch(opts.image, opts.pattern, opts.amount),
        Match(opts) => matching(opts.pattern),
    }
}

fn fetch(image: ImageName, regex: Option<Regex>, amount: u32) -> Result<(), Box<dyn Error>> {
    let tags = DockerHubTagFetcher::fetch(
        image,
        Page {
            size: amount,
            page: 1,
        },
    )?;

    let result = if let Some(pattern) = regex {
        let extractor = VersionExtractor::from(pattern);
        let result: Vec<String> = extractor.filter(tags).collect();
        println!(
            "Fetched {} tags. Found {} matching `{}`:",
            amount,
            result.len(),
            extractor
        );
        result
    } else {
        println!("Fetched {} tags:", tags.len());
        tags
    };

    println!("{}", result.join("\n"));

    Ok(())
}

fn matching(pattern: Regex) -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let info_result: Result<Vec<Vec<Info>>, Box<dyn Error>> = stdin
        .lock()
        .lines()
        .map(|line| Ok(Info::extract_from(line?)?))
        .collect();

    let cli_extractor = VersionExtractor::from(pattern);
    let result: Result<Vec<Option<String>>, Box<dyn Error>> = info_result?
        .into_iter()
        .flatten()
        .map(|info| {
            let tags = DockerHubTagFetcher::fetch(info.image, Page { size: 25, page: 1 })?;

            let maybe_newest = match info.regex {
                Some(regex) => VersionExtractor::from(regex).max(tags)?,
                None => cli_extractor.max(tags)?,
            };
            Ok(maybe_newest)
        })
        .collect();
    let matches: Vec<String> = result?.into_iter().filter_map(|tag| tag).collect();
    println!("Newest tags: {:#?}", matches);

    Ok(())
}
