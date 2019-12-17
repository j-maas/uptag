use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use env_logger;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json;
use structopt::StructOpt;

use updock::FromStatement;
use updock::{DockerHubTagFetcher, Page, TagFetcher};
use updock::{Image, ImageName};
use updock::{Version, VersionExtractor};

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
    default_pattern: Option<VersionExtractor>,
    #[structopt(short, long)]
    json: bool,
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
        Fetch(opts) => fetch(opts),
        Check(opts) => check(opts),
        Upgrade(opts) => Ok(()),
    }
}

fn fetch(opts: FetchOpts) -> Result<()> {
    let fetcher = DockerHubTagFetcher::new();
    let tags = fetcher
        .fetch(&opts.image, &Page::first(opts.amount))
        .context("Failed to fetch tags.")?;

    let result = if let Some(extractor) = opts.pattern {
        let result: Vec<String> = extractor.filter(tags).collect();
        println!(
            "Fetched {} tags. Found {} matching `{}`:",
            opts.amount,
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

fn check(opts: CheckOpts) -> Result<()> {
    let stdin = io::stdin();
    let mut input = String::new();
    stdin
        .lock()
        .read_to_string(&mut input)
        .context("Failed to read from stdin.")?;

    let amount = 25;
    let result = FromStatement::iter(&input)
        .context("Failed to parse a statement.")?
        .into_iter()
        .map(|statement| {
            let image = statement.image();
            let extractor = statement
                .extractor()
                .as_ref()
                .or_else(|| opts.default_pattern.as_ref())
                .ok_or_else(|| anyhow!("Failed to find version pattern for {}. Please specify it either in an annotation or give a default version pattern.", image))?;
            let upgrade = check_statement(&image, extractor, statement.breaking_degree(), amount)
                .with_context(|| format!("Failed to check {}.", statement.image()))?;
            Ok((upgrade, image, extractor.to_string()))
        })
        .collect::<Result<Vec<_>>>();
    let upgrades = result?;

    let output = if opts.json {
        let map = upgrades
            .into_iter()
            .map(|(upgrade, image, _)| (image.to_string(), upgrade))
            .collect::<HashMap<_, _>>();
        serde_json::to_string_pretty(&map).context("Failed to serialize result.")?
    } else {
        upgrades
            .into_iter()
            .map(|(upgrade, image, pattern)| match upgrade {
                Upgrade {
                    compatible: Some(compatible),
                    breaking: Some(breaking),
                } => format!(
                    "{} can upgrade to `{}`, and has breaking upgrade to `{}`.",
                    image, compatible, breaking
                ),
                Upgrade {
                    compatible: Some(compatible),
                    breaking: None,
                } => format!("{} can upgrade to `{}`.", image, compatible),
                Upgrade {
                    compatible: None,
                    breaking: Some(breaking),
                } => format!("{} has breaking upgrade to `{}`.", image, breaking),
                Upgrade {
                    compatible: None,
                    breaking: None,
                } => format!(
                    "{} has no upgrade matching `{}` in the latest {} tags.",
                    image, pattern, amount
                ),
            })
            .join("\n")
    };

    println!("{}", output);

    Ok(())
}

fn check_statement(
    image: &Image,
    extractor: &VersionExtractor,
    breaking_degree: usize,
    amount: usize,
) -> Result<Upgrade> {
    let fetcher = DockerHubTagFetcher::new();
    let tags = fetcher
        .fetch(&image.name, &Page::first(amount))
        .context("Failed to fetch tags.")?;
    let current_version = extractor.extract_from(&image.tag).unwrap();
    let (compatible, breaking): (Vec<(Version, String)>, Vec<(Version, String)>) = extractor
        .extract(tags)
        .partition(|(candidate, _)| current_version.upgrades_to(candidate, breaking_degree));

    let max_compatible = compatible
        .into_iter()
        .filter(|(candidate, _)| candidate > &current_version)
        .max()
        .map(|(_, tag)| tag);
    let max_breaking = breaking
        .into_iter()
        .filter(|(candidate, _)| candidate > &current_version)
        .max()
        .map(|(_, tag)| tag);
    Ok(Upgrade {
        compatible: max_compatible,
        breaking: max_breaking,
    })
}

type Tag = String;

#[derive(Debug, Serialize, Deserialize)]
struct Upgrade {
    compatible: Option<Tag>,
    breaking: Option<Tag>,
}
