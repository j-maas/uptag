use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;

use env_logger;
use structopt::StructOpt;

use updock::ImageName;
use updock::Info;
use updock::VersionExtractor;
use updock::{DockerHubTagFetcher, Page, TagFetcher};

#[derive(Debug, StructOpt)]
enum Opts {
    Fetch(FetchOpts),
    Check(CheckOpts),
    Upgrade(UpgradeOpts),
}

#[derive(Debug, StructOpt)]
struct FetchOpts {
    #[structopt(short, long)]
    image: ImageName,
    #[structopt(short, long)]
    pattern: Option<VersionExtractor>,
    #[structopt(short, long, default_value = "25")]
    amount: u32,
}

#[derive(Debug, StructOpt)]
struct CheckOpts {
    #[structopt(short, long)]
    default_pattern: VersionExtractor,
}

#[derive(Debug, StructOpt)]
struct UpgradeOpts {
    #[structopt(short, long, parse(from_os_str))]
    input: PathBuf,
    #[structopt(short, long)]
    default_pattern: VersionExtractor,
}

fn main() -> Result<()> {
    env_logger::init();

    let opts = Opts::from_args();

    use Opts::*;
    match opts {
        Fetch(opts) => fetch(&opts.image, opts.pattern, opts.amount),
        Check(opts) => check(&opts.default_pattern),
        Upgrade(opts) => upgrade(opts.input, opts.default_pattern),
    }
}

fn fetch(image: &ImageName, pattern: Option<VersionExtractor>, amount: u32) -> Result<()> {
    let tags = DockerHubTagFetcher::fetch(
        &image,
        &Page {
            size: amount,
            page: 1,
        },
    )?;

    let result = if let Some(extractor) = pattern {
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

fn check(default_extractor: &VersionExtractor) -> Result<()> {
    let stdin = io::stdin();
    let lines: std::result::Result<Vec<String>, io::Error> = stdin.lock().lines().collect();
    let lines = lines?;

    let amount = 25;
    let result = extract(lines, amount, &default_extractor)?;

    let output: Vec<String> = result
        .into_iter()
        .map(|(maybe_tag, info)| match maybe_tag {
            Some(tag) => format!(
                "Current: `{}:{}`. Newest matching tag: `{}`.",
                info.image, info.tag, tag
            ),
            None => {
                let pattern = match info.extractor {
                    Some(extractor) => extractor.to_string(),
                    None => default_extractor.to_string(),
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

fn upgrade(dockerfile: PathBuf, default_extractor: VersionExtractor) -> Result<()> {
    let input = std::fs::read_to_string(&dockerfile)?;

    let updates = extract(input.lines(), 25, &default_extractor)?;
    let replacements: HashMap<ImageName, String> = updates
        .iter()
        .filter_map(|(maybe_tag, info)| {
            maybe_tag
                .as_ref()
                .map(|tag| (info.image.clone(), tag.clone()))
        })
        .collect();
    let output = Info::replace(input, replacements);

    std::fs::write(&dockerfile, output)?;

    Ok(())
}

fn extract(
    lines: impl IntoIterator<Item = impl AsRef<str>>,
    amount: u32,
    default_extractor: &VersionExtractor,
) -> Result<Vec<(Option<String>, Info)>> {
    type MaybeExtraction = Option<(Option<String>, Info)>;
    let results: Result<Vec<MaybeExtraction>> = lines
        .into_iter()
        .map(|line| {
            let extracted: Option<Result<(Option<String>, Info)>> =
                Info::extract_from(line)?.map(|info| {
                    let tags = DockerHubTagFetcher::fetch(
                        &info.image,
                        &Page {
                            size: amount,
                            page: 1,
                        },
                    )?;

                    let maybe_newest = match &info.extractor {
                        Some(extractor) => extractor.max(tags)?,
                        None => default_extractor.max(tags)?,
                    };
                    Ok((maybe_newest, info))
                });
            Ok(extracted.transpose()?)
        })
        .collect();
    Ok(results?.into_iter().filter_map(|info| info).collect())
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;
