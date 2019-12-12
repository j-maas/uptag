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
    Check(CheckOpts),
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
struct CheckOpts {
    #[structopt(short, long)]
    default_pattern: Regex,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let opts = Opts::from_args();

    use Opts::*;
    match opts {
        Fetch(opts) => fetch(opts.image, opts.pattern, opts.amount),
        Check(opts) => check(opts.default_pattern),
    }
}

fn fetch(image: ImageName, regex: Option<Regex>, amount: u32) -> Result<(), Box<dyn Error>> {
    let tags = DockerHubTagFetcher::fetch(
        &image,
        &Page {
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

fn check(default_pattern: Regex) -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let info_result: Result<Vec<Vec<Info>>, Box<dyn Error>> = stdin
        .lock()
        .lines()
        .map(|line| Ok(Info::extract_from(line?)?))
        .collect();

    let cli_extractor = VersionExtractor::from(default_pattern);
    let amount = 25;
    type ExtractionResult = Result<Vec<(Option<String>, Info)>, Box<dyn Error>>;
    let result: ExtractionResult = info_result?
        .into_iter()
        .flatten()
        .map(|info| {
            let tags = DockerHubTagFetcher::fetch(
                &info.image,
                &Page {
                    size: amount,
                    page: 1,
                },
            )?;

            let maybe_newest = match &info.extractor {
                Some(extractor) => extractor.max(tags)?,
                None => cli_extractor.max(tags)?,
            };
            Ok((maybe_newest, info))
        })
        .collect();
    let output: Vec<String> = result?
        .into_iter()
        .map(|(maybe_tag, info)| match maybe_tag {
            Some(tag) => format!(
                "Current: `{}:{}`. Newest matching tag: `{}`.",
                info.image, info.tag, tag
            ),
            None => {
                let pattern = match info.extractor {
                    Some(extractor) => extractor.to_string(),
                    None => cli_extractor.to_string(),
                };
                format!(
                    "Current: `{}:{}`. No tag matching `{}` found. (Searched latest {} tags.)",
                    info.image, info.tag, pattern, amount
                )
            }
        })
        .collect();
    println!(
        "Found {} parent images:\n{}",
        output.len(),
        output.join("\n")
    );

    Ok(())
}
