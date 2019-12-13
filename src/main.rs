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
    amount: usize,
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

fn fetch(image: &ImageName, pattern: Option<VersionExtractor>, amount: usize) -> Result<()> {
    let fetcher = DockerHubTagFetcher::new();
    let tags = fetcher.fetch(&image, &Page::first(amount))?;

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
    let mut input = String::new();
    stdin.lock().read_to_string(&mut input)?;

    let result: Result<Vec<String>> = FromStatement::extract_all(input)?
        .into_iter()
        .map(|statement| check_statement(&statement, default_extractor))
        .collect();
    let output = result?;

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
    let fetcher = DockerHubTagFetcher::new();
    let tags = fetcher.fetch(&statement.image, &Page::first(25))?;
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

    let output: String = FromStatement::replace_all(input, |statement_result| {
        statement_result
            .map(
                |statement| match process_statement(statement.clone(), &default_extractor) {
                    Some(update) => format!("{}", update),
                    None => {
                        eprintln!("Keeping current tag (`{}`).", statement.tag);
                        format!("{}", statement)
                    }
                },
            )
            .unwrap_or_else(|(e, line)| {
                eprintln!("Error upgrading Dockerfile: {}", e);
                line
            })
    });

    std::fs::write(&dockerfile, output)?;

    Ok(())
}

fn process_statement(
    mut statement: FromStatement,
    default_extractor: &VersionExtractor,
) -> Option<FromStatement> {
    let amount = 25;
    let fetcher = DockerHubTagFetcher::new();
    let tags = fetcher.fetch(&statement.image, &Page::first(amount));
    let tags = match tags {
        Err(err) => {
            eprintln!("Error fetching tags for image {}: {}", statement.image, err);
            return None;
        }
        Ok(tags) => tags,
    };
    let extractor = statement.extractor.as_ref().unwrap_or(default_extractor);
    let newest = extractor.max(tags);
    let newest = match newest {
        Err(err) => {
            eprintln!(
                "Error extracting version from tags for image {}: {}",
                statement.image, err
            );
            return None;
        }
        Ok(newest) => newest,
    };
    match newest {
        None => {
            let pattern = format!("{}", extractor);
            eprintln!(
                "The latest {} tags for image {} did not contain any match for `{}`.",
                statement.image, pattern, amount
            );
            None
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
                Some(statement)
            } else {
                println!(
                    "Image {} has a breaking upgrade from `{}` to `{}`.",
                    statement.image, statement.tag, update
                );
                None
            }
        }
    }
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;
