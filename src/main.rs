use std::collections::HashMap;
use std::fs;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use env_logger;
use indexmap::IndexMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json;
use structopt::StructOpt;

use updock::Matches;
use updock::{DockerHubTagFetcher, TagFetcher};
use updock::{Image, ImageName};
use updock::{Update, Updock};
use updock::{VersionExtractor, VersionTag};

#[derive(Debug, StructOpt)]
enum Opts {
    Fetch(FetchOpts),
    Check(CheckOpts),
    CheckCompose(CheckComposeOpts),
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
struct CheckComposeOpts {
    #[structopt(short, long, parse(from_os_str))]
    file: PathBuf,
    #[structopt(flatten)]
    check_opts: CheckOpts,
}

fn main() -> Result<()> {
    env_logger::init();

    let opts = Opts::from_args();

    use Opts::*;
    match opts {
        Fetch(opts) => fetch(opts),
        Check(opts) => check(opts),
        CheckCompose(opts) => check_compose(opts),
    }
}

fn fetch(opts: FetchOpts) -> Result<()> {
    let fetcher = DockerHubTagFetcher::new();
    let tags = fetcher
        .fetch(&opts.image, opts.amount)
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
    let updock = Updock::default();
    let upgrades = check_input(&updock, input, &opts.default_pattern, amount)?;

    let output = if opts.json {
        let map = upgrades
            .into_iter()
            .map(|(image, upgrade, _)| (image.to_string(), upgrade))
            .collect::<IndexMap<_, _>>(); // IndexMap preserves order.
        serde_json::to_string_pretty(&map).context("Failed to serialize result.")?
    } else {
        upgrades
            .into_iter()
            .map(|(image, upgrade, pattern)| match upgrade {
                Some(Update::Both {
                    compatible,
                    breaking,
                }) => format!(
                    "{} can upgrade to `{}`, and has breaking upgrade to `{}`.",
                    image, compatible, breaking
                ),
                Some(Update::Compatible(compatible)) => {
                    format!("{} can upgrade to `{}`.", image, compatible)
                }
                Some(Update::Breaking(breaking)) => {
                    format!("{} has breaking upgrade to `{}`.", image, breaking)
                }
                None => format!(
                    "{} has no upgrade matching `{}` in the latest {} tags.",
                    image, pattern, amount
                ),
            })
            .join("\n")
    };

    println!("{}", output);

    Ok(())
}

fn check_input<T>(
    updock: &Updock<T>,
    input: String,
    default_extractor: &Option<VersionExtractor>,
    amount: usize,
) -> Result<Vec<(Image, Option<Update>, String)>>
where
    T: TagFetcher + std::fmt::Debug + 'static,
    T::Error: 'static + Send + Sync,
{
    Matches::iter(&input)
        .map(|statement| {
            let image = statement.image();
            let statement_extractor = statement.extractor().transpose()?;
            let extractor = statement_extractor
                .as_ref()
                .or_else(|| default_extractor.as_ref())
                .ok_or_else(|| anyhow!(
                    "Failed to find version pattern for {}. Please specify it either in an annotation or give a default version pattern.",
                     image
                    )
                )?;
            let current_version = VersionTag::from(extractor, image.tag.clone())
                .ok_or_else(|| {
                    anyhow!(
                        "The current tag `{}` does not match the pattern `{}`.",
                        image.tag,
                        extractor.as_str()
                    )
                })?;
            let upgrade = updock
                .check_update(
                    &image.name,
                    &current_version,
                    extractor,
                    statement.breaking_degree(),
                    amount
                )
                .with_context(|| format!("Failed to check {}.", statement.image()))?;
            Ok((image, upgrade, extractor.to_string()))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct DockerCompose {
    services: IndexMap<String, Service>, // IndeMap preserves order.
}

#[derive(Debug, Serialize, Deserialize)]
struct Service {
    build: PathBuf,
}

fn check_compose(opts: CheckComposeOpts) -> Result<()> {
    let compose_file = fs::File::open(&opts.file)
        .with_context(|| format!("Failed to read file `{}`.", opts.file.display()))?;
    let compose: DockerCompose =
        serde_yaml::from_reader(compose_file).context("Failed to parse Docker Compose file.")?;

    let compose_dir = opts.file.parent().unwrap();
    let amount = 25;
    let updock = Updock::default();
    let result = compose
        .services
        .into_iter()
        .map(|(service_name, service)| {
            let path = compose_dir.join(service.build).join("Dockerfile");
            let input = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read file `{}`.", path.display()))?;
            let upgrades = check_input(&updock, input, &opts.check_opts.default_pattern, amount)?;

            Ok((service_name, upgrades))
        })
        .collect::<Result<Vec<_>>>();
    let services = result?;

    let output = if opts.check_opts.json {
        let map = services
            .into_iter()
            .map(|(service, upgrades)| {
                (
                    service,
                    upgrades
                        .into_iter()
                        .map(|(image, upgrade, _)| (image.to_string(), upgrade))
                        .collect(),
                )
            })
            .collect::<HashMap<_, HashMap<_, _>>>();
        serde_json::to_string_pretty(&map).context("Failed to serialize result.")?
    } else {
        services
            .into_iter()
            .map(|(service, upgrades)| {
                let upgrades_output = upgrades
                    .into_iter()
                    .map(|(image, upgrade, pattern)| match upgrade {
                        Some(Update::Both {
                            compatible,
                            breaking,
                        }) => format!(
                            "{} can upgrade to `{}`, and has breaking upgrade to `{}`.",
                            image, compatible, breaking
                        ),
                        Some(Update::Compatible(compatible)) => {
                            format!("{} can upgrade to `{}`.", image, compatible)
                        }
                        Some(Update::Breaking(breaking)) => {
                            format!("{} has breaking upgrade to `{}`.", image, breaking)
                        }
                        None => format!(
                            "{} has no upgrade matching `{}` in the latest {} tags.",
                            image, pattern, amount
                        ),
                    })
                    .join("\n");
                format!("Service {}:\n{}", service, upgrades_output)
            })
            .join("\n\n")
    };

    println!("{}", output);

    Ok(())
}
