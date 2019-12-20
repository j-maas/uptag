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

use updock::{
    CheckError, DockerHubTagFetcher, Image, ImageName, PatternInfo, TagFetcher, Update, Updock,
    VersionExtractor,
};

#[derive(Debug, StructOpt)]
enum Opts {
    Fetch(FetchOpts),
    Check(CheckOpts),
    CheckCompose(CheckComposeOpts),
}

#[derive(Debug, StructOpt)]
struct FetchOpts {
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
    #[structopt(parse(from_os_str))]
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
        .context("Failed to fetch tags")?;

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
        .context("Failed to read from stdin")?;

    let amount = 25;
    let updock = Updock::default();
    let updates = updock.check_input(&input, amount);

    if opts.json {
        let map = updates
            .map(|(image_name, update_result)| {
                (
                    image_name.to_string(),
                    update_result
                        .map_err(|error| format!("{:#}", anyhow::Error::new(error)))
                        .map(|(maybe_update, _)| maybe_update),
                )
            })
            .collect::<IndexMap<_, _>>(); // IndexMap preserves order.
        println!(
            "{}",
            serde_json::to_string_pretty(&map).context("Failed to serialize result.")?
        )
    } else {
        println!("{}", display_updates(updates, amount))
    }

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
        .with_context(|| format!("Failed to read file `{}`", opts.file.display()))?;
    let compose: DockerCompose =
        serde_yaml::from_reader(compose_file).context("Failed to parse Docker Compose file.")?;

    let compose_dir = opts.file.parent().unwrap();
    let amount = 25;
    let updock = Updock::default();
    let services = compose.services.into_iter().map(|(service_name, service)| {
        (service_name, {
            let path = compose_dir.join(service.build).join("Dockerfile");
            fs::read_to_string(&path)
                .with_context(|| format!("Failed to read file `{}`", path.display()))
                .map(|input| updock.check_input(&input, amount).collect::<Vec<_>>())
        })
    });

    if opts.check_opts.json {
        let map = services
            .map(|(service, result)| {
                (
                    service,
                    result
                        .map_err(|error| format!("{:#}", error))
                        .map(|updates| {
                            updates
                                .into_iter()
                                .map(|(image_name, updates_result)| {
                                    (
                                        image_name.to_string(),
                                        updates_result
                                            .map_err(|error| {
                                                format!("{:#}", anyhow::Error::new(error))
                                            })
                                            .map(|(maybe_update, _)| maybe_update),
                                    )
                                })
                                .collect()
                        }),
                )
            })
            .collect::<IndexMap<_, Result<IndexMap<_, _>, String>>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&map).context("Failed to serialize result")?
        )
    } else {
        for (service, result) in services {
            let updates_output = match result {
                Ok(updates) => {
                    let updates_output = display_updates(updates, amount);
                    updates_output.to_string()
                }
                Err(error) => format!("{:#}", error),
            };
            println!("Service {}:\n{}\n", service, updates_output)
        }
    }

    Ok(())
}

fn display_updates(
    updates: impl IntoIterator<
        Item = (
            Image,
            Result<(Option<Update>, PatternInfo), CheckError<DockerHubTagFetcher>>,
        ),
    >,
    amount: usize,
) -> String {
    updates
        .into_iter()
        .map(|(image, update_result)| {
            update_result
                .map(|(update, pattern_info)| match update {
                    Some(Update::Both {
                        compatible,
                        breaking,
                    }) => format!(
                        "{} has update to `{}`, and has breaking update to `{}`.",
                        image, compatible, breaking
                    ),
                    Some(Update::Compatible(compatible)) => {
                        format!("{} has update to `{}`.", image, compatible)
                    }
                    Some(Update::Breaking(breaking)) => {
                        format!("{} has breaking update to `{}`.", image, breaking)
                    }
                    None => format!(
                        "{} has no update matching `{}` in the latest {} tags.",
                        image, pattern_info.extractor, amount
                    ),
                })
                .unwrap_or_else(|error| {
                    dbg!(&error);
                    format!(
                        "Failed to check image {}: {:#}",
                        image.name,
                        anyhow::Error::new(error)
                    )
                })
        })
        .join("\n")
}
