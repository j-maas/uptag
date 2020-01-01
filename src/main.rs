use std::fs;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;

use anyhow::{Context, Result};
use env_logger;
use indexmap::IndexMap;
use serde_json::json;
use serde_yaml;
use structopt::StructOpt;

use updock::docker_compose::DockerCompose;
use updock::image::{Image, ImageName};
use updock::tag_fetcher::{DockerHubTagFetcher, TagFetcher};
use updock::version_extractor::VersionExtractor;
use updock::{CheckError, DockerfileReport, PatternInfo, Update, Updock};

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
        let report = DockerfileReport::from(updates);
        let successes = report
            .successes
            .into_iter()
            .map(|(image, (update, _))| (image, update))
            .collect::<Vec<_>>();
        let failures = report
            .failures
            .into_iter()
            .map(|(image, error)| (image, format!("{:#}", anyhow::Error::new(error))))
            .collect::<Vec<_>>();

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "successes": successes,
                "failures": failures
            }))
            .context("Failed to serialize result.")?
        )
    } else {
        for update_result in updates {
            match display_update(update_result, amount) {
                Ok(output) => println!("{}", output),
                Err(failure) => eprintln!("{}", failure),
            }
        }
    }

    Ok(())
}

fn check_compose(opts: CheckComposeOpts) -> Result<()> {
    let compose_file = fs::File::open(&opts.file)
        .with_context(|| format!("Failed to read file `{}`", opts.file.display()))?;
    let compose: DockerCompose =
        serde_yaml::from_reader(compose_file).context("Failed to parse Docker Compose file")?;

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
            match result {
                Ok(updates) => {
                    println!("Service `{}`:", service);
                    for update_result in updates {
                        match display_update(update_result, amount) {
                            Ok(output) => println!("{}", output),
                            Err(failure) => eprintln!("{}", failure),
                        }
                    }
                }
                Err(error) => eprintln!("Failed to check service `{}`: {:#}", service, error),
            };
            println!();
        }
    }

    Ok(())
}

fn display_update(
    (image, update_result): (
        Image,
        Result<(Option<Update>, PatternInfo), CheckError<DockerHubTagFetcher>>,
    ),
    amount: usize,
) -> Result<String, String> {
    update_result
        .map_err(|error| format!("Failed to check `{}`: {:#}", image, error))
        .map(|(maybe_update, pattern_info)| match maybe_update {
            None => format!(
                "`{}` has no update matching `{}` in the latest {} tags.",
                image, pattern_info.extractor, amount
            ),
            Some(Update::Both {
                compatible,
                breaking,
            }) => format!(
                "`{}` has compatible update to `{}:{}` and breaking update to `{}:{}`.",
                image, image.name, compatible, image.name, breaking
            ),
            Some(Update::Breaking(breaking)) => format!(
                "`{}` has breaking update to `{}:{}`.",
                image, image.name, breaking
            ),
            Some(Update::Compatible(compatible)) => format!(
                "`{}` has compatible update to `{}:{}`.",
                image, image.name, compatible
            ),
        })
}
