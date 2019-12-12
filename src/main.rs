use std::error::Error;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;

use env_logger;
use structopt::StructOpt;

use updock::FromStatement;
use updock::ImageName;
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
        .map(|(maybe_tag, statement)| match maybe_tag {
            Some(tag) => format!(
                "Current: `{}:{}`. Newest matching tag: `{}`.",
                statement.image, statement.tag, tag
            ),
            None => {
                let pattern = match statement.extractor {
                    Some(extractor) => extractor.to_string(),
                    None => default_extractor.to_string(),
                };
                format!(
                    "Current: `{}:{}`. No tag matching `{}` found. (Searched latest {} tags.)",
                    statement.image, statement.tag, pattern, amount
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

    let amount = 25;
    let output = map_extraction(
        input.lines(),
        amount,
        &default_extractor,
        |line, maybe_extraction| match maybe_extraction {
            None => line,
            Some(extraction) => match extraction.newest_tag {
                None => {
                    let pattern = match &extraction.statement.extractor {
                        Some(extractor) => format!("{}", extractor),
                        None => format!("{}", default_extractor),
                    };
                    eprintln!("Could not find any tag for image {} matching `{}`. (Searched the latest {} tags.) Current tag `{}` will be kept.", extraction.statement.image,pattern, amount, extraction.statement.tag);
                    format!("{}", extraction.statement)
                }
                Some(update) => {
                    let mut statement = extraction.statement;
                    println!(
                        "Upgrading image {} from `{}` to `{}`.",
                        statement.image, statement.tag, update
                    );
                    statement.tag = update;
                    format!("{}", statement)
                }
            },
        },
    )?;

    std::fs::write(&dockerfile, output.join("\n"))?;

    Ok(())
}

struct ExtractionFromStatement {
    statement: FromStatement,
    newest_tag: Option<String>,
}

fn extract(
    lines: impl IntoIterator<Item = impl AsRef<str>>,
    amount: u32,
    default_extractor: &VersionExtractor,
) -> Result<Vec<(Option<String>, FromStatement)>> {
    Ok(
        map_extraction(lines, amount, default_extractor, |_, maybe_extraction| {
            maybe_extraction.map(|extraction| (extraction.newest_tag, extraction.statement))
        })?
        .into_iter()
        .filter_map(|statement| statement)
        .collect(),
    )
}

fn map_extraction<T>(
    lines: impl IntoIterator<Item = impl AsRef<str>>,
    amount: u32,
    default_extractor: &VersionExtractor,
    mut process: impl FnMut(String, Option<ExtractionFromStatement>) -> T,
) -> Result<Vec<T>> {
    lines
        .into_iter()
        .map(|line| {
            let extracted: Result<Option<ExtractionFromStatement>> =
                FromStatement::extract_from(&line)?
                    .map(|statement| {
                        let tags = DockerHubTagFetcher::fetch(
                            &statement.image,
                            &Page {
                                size: amount,
                                page: 1,
                            },
                        )?;

                        let newest_tag = match &statement.extractor {
                            Some(extractor) => extractor.max(tags)?,
                            None => default_extractor.max(tags)?,
                        };
                        Ok(ExtractionFromStatement {
                            statement,
                            newest_tag,
                        })
                    })
                    .transpose();
            Ok(process(line.as_ref().to_string(), extracted?))
        })
        .collect()
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;
