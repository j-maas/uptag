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

    let result: Result<Vec<Option<String>>> = lines
        .into_iter()
        .map(|line| {
            Ok(FromStatement::extract_from(line)?
                .map(|statement| check_statement(&statement, default_extractor))
                .transpose()?)
        })
        .collect();
    let output: Vec<String> = result?.into_iter().filter_map(|s| s).collect();

    println!(
        "Found {} parent images:\n{}",
        output.len(),
        output.join("\n")
    );

    Ok(())
}

fn check_statement(
    statement: &FromStatement,
    default_extractor: &VersionExtractor,
) -> Result<String> {
    let amount = 25;
    let tags = DockerHubTagFetcher::fetch(
        &statement.image,
        &Page {
            size: amount,
            page: 1,
        },
    )?;
    let newest = match &statement.extractor {
        Some(extractor) => extractor.max(tags)?,
        None => default_extractor.max(tags)?,
    };
    let output = match newest {
        Some(tag) => format!(
            "Current: `{}:{}`. Newest matching tag: `{}`.",
            statement.image, statement.tag, tag
        ),
        None => {
            let pattern = match &statement.extractor {
                Some(extractor) => extractor.to_string(),
                None => default_extractor.to_string(),
            };
            format!(
                "Current: `{}:{}`. No tag matching `{}` found. (Searched latest {} tags.)",
                statement.image, statement.tag, pattern, amount
            )
        }
    };
    Ok(output)
}

fn upgrade(dockerfile: PathBuf, default_extractor: VersionExtractor) -> Result<()> {
    let input = std::fs::read_to_string(&dockerfile)?;

    let result: Result<Vec<String>> = input
        .lines()
        .map(|line| {
            Ok(FromStatement::extract_from(&line)?
                .map(|mut statement| process_statement(&mut statement, &default_extractor))
                .transpose()?
                .unwrap_or_else(|| line.to_string()))
        })
        .collect();
    let output: Vec<String> = result?;

    std::fs::write(&dockerfile, output.join("\n"))?;

    Ok(())
}

fn process_statement(
    mut statement: &mut FromStatement,
    default_extractor: &VersionExtractor,
) -> Result<String> {
    let amount = 25;
    let tags = DockerHubTagFetcher::fetch(
        &statement.image,
        &Page {
            size: amount,
            page: 1,
        },
    )?;
    let extractor = statement.extractor.as_ref().unwrap_or(default_extractor);
    let newest = extractor.max(tags)?;
    let output = match newest {
        None => {
            let pattern = format!("{}", extractor);
            eprintln!("The latest {} tags for image {} did not contain any match for `{}`. Current tag `{}` will be kept.", statement.image,pattern, amount, statement.tag);
            format!("{}", statement)
        }
        Some(update) => {
            let current = extractor.extract_from(&statement.tag).unwrap().unwrap();
            let next = extractor.extract_from(&update).unwrap().unwrap();
            if current.should_upgrade_to(next, statement.breaking_degree) {
                println!(
                    "Upgrading image {} from `{}` to `{}`.",
                    statement.image, statement.tag, update
                );
                statement.tag = update;
                format!("{}", statement)
            } else {
                println!(
                    "Image {} has a breaking upgrade from `{}` to `{}`. Will keep current tag (`{1}`).",
                    statement.image, statement.tag, update
                );
                format!("{}", statement)
            }
        }
    };
    Ok(output)
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;
