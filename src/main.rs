use std::collections::HashMap;
use std::fs;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;

use anyhow::{Context, Result};
use env_logger;
use indexmap::IndexMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json;
use structopt::StructOpt;

use updock::ImageName;
use updock::VersionExtractor;
use updock::{DockerHubTagFetcher, TagFetcher};
use updock::{ImageInfo, Update, Updock};

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
    let updates = updock
        .check_input(&input, amount)
        .filter_map(|result| match result {
            Ok(res) => Some(res),
            Err(error) => {
                eprintln!("{:#}", anyhow::Error::new(error));
                None
            }
        })
        .collect::<Vec<_>>();

    let output = if opts.json {
        let map = updates
            .into_iter()
            .map(|(info, update)| (info.image.to_string(), update))
            .collect::<IndexMap<_, _>>(); // IndexMap preserves order.
        serde_json::to_string_pretty(&map).context("Failed to serialize result.")?
    } else {
        display_updates(updates.into_iter(), amount)
    };

    println!("{}", output);

    Ok(())
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
            let updates = updock
                .check_input(&input, amount)
                .filter_map(|result| match result {
                    Ok(res) => Some(res),
                    Err(error) => {
                        eprintln!("{:#}", anyhow::Error::new(error));
                        None
                    }
                })
                .collect::<Vec<_>>();

            Ok((service_name, updates))
        })
        .collect::<Result<Vec<_>>>();
    let services = result?;

    let output = if opts.check_opts.json {
        let map = services
            .into_iter()
            .map(|(service, updates)| {
                (
                    service,
                    updates
                        .into_iter()
                        .map(|(info, update)| (info.image.to_string(), update))
                        .collect(),
                )
            })
            .collect::<HashMap<_, HashMap<_, _>>>();
        serde_json::to_string_pretty(&map).context("Failed to serialize result.")?
    } else {
        services
            .into_iter()
            .map(|(service, updates)| {
                let updates_output = display_updates(updates.into_iter(), amount);
                format!("Service {}:\n{}", service, updates_output)
            })
            .join("\n\n")
    };

    println!("{}", output);

    Ok(())
}

fn display_updates(
    updates: impl Iterator<Item = (ImageInfo, Option<Update>)>,
    amount: usize,
) -> String {
    updates
        .map(|(info, update)| match update {
            Some(Update::Both {
                compatible,
                breaking,
            }) => format!(
                "{} can update to `{}`, and has breaking update to `{}`.",
                info.image, compatible, breaking
            ),
            Some(Update::Compatible(compatible)) => {
                format!("{} can update to `{}`.", info.image, compatible)
            }
            Some(Update::Breaking(breaking)) => {
                format!("{} has breaking update to `{}`.", info.image, breaking)
            }
            None => format!(
                "{} has no update matching `{}` in the latest {} tags.",
                info.image, info.extractor, amount
            ),
        })
        .join("\n")
}
