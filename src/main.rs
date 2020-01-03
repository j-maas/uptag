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

use updock::docker_compose::{DockerCompose, DockerComposeReport};
use updock::image::ImageName;
use updock::tag_fetcher::{DockerHubTagFetcher, TagFetcher};
use updock::version_extractor::VersionExtractor;
use updock::{DockerfileReport, DockerfileResult, Update, Updock};

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
    let result = match opts {
        Fetch(opts) => fetch(opts),
        Check(opts) => check(opts),
        CheckCompose(opts) => check_compose(opts),
    };

    match result {
        Ok(code) => code.exit(),
        Err(error) => {
            eprintln!("{:#}", error);
            EXIT_ERROR.exit();
        }
    }
}

struct ExitCode(i32);

const EXIT_OK: ExitCode = ExitCode(0);
const EXIT_NO_UPDATE: ExitCode = ExitCode(0);
const EXIT_COMPATIBLE_UPDATE: ExitCode = ExitCode(1);
const EXIT_BREAKING_UPDATE: ExitCode = ExitCode(2);
const EXIT_ERROR: ExitCode = ExitCode(10);

impl ExitCode {
    fn from(maybe_update: &Option<Update>) -> Self {
        match maybe_update {
            None => EXIT_NO_UPDATE,
            Some(Update::Compatible(_)) => EXIT_COMPATIBLE_UPDATE,
            Some(Update::Breaking(_)) => EXIT_BREAKING_UPDATE,
            Some(Update::Both { .. }) => EXIT_BREAKING_UPDATE,
        }
    }

    fn merge(&mut self, other: &ExitCode) {
        self.0 = std::cmp::max(self.0, other.0)
    }

    fn exit(&self) -> ! {
        std::process::exit(self.0)
    }
}

fn fetch(opts: FetchOpts) -> Result<ExitCode> {
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

    Ok(EXIT_OK)
}

fn check(opts: CheckOpts) -> Result<ExitCode> {
    let stdin = io::stdin();
    let mut input = String::new();
    stdin
        .lock()
        .read_to_string(&mut input)
        .context("Failed to read from stdin")?;

    let amount = 25;
    let updock = Updock::default();
    let updates = updock.check_input(&input, amount);

    let mut exit_code = EXIT_NO_UPDATE;

    if opts.json {
        let report = DockerfileReport::from(updates);

        let no_updates = report
            .no_updates()
            .map(|(image, _)| image.to_string())
            .collect::<Vec<_>>();
        let compatible_updates = report
            .compatible_updates()
            .map(|(image, tag, _)| (image.to_string(), tag.clone()))
            .collect::<IndexMap<_, _>>();
        let breaking_updates = report
            .breaking_updates()
            .map(|(image, tag, _)| (image.to_string(), tag.clone()))
            .collect::<IndexMap<_, _>>();

        let failures = report
            .failures
            .into_iter()
            .map(|(image, error)| {
                (
                    image.to_string(),
                    format!("{:#}", anyhow::Error::new(error)),
                )
            })
            .collect::<IndexMap<_, _>>();

        if !compatible_updates.is_empty() {
            exit_code = EXIT_COMPATIBLE_UPDATE;
        }
        if !breaking_updates.is_empty() {
            exit_code = EXIT_BREAKING_UPDATE;
        }
        if !failures.is_empty() {
            exit_code = EXIT_ERROR;
        }

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "failures": failures,
                "breaking_updates": breaking_updates,
                "compatible_updates": compatible_updates,
                "no_updates": no_updates
            }))
            .context("Failed to serialize result.")?
        );
    } else {
        let report = DockerfileReport::from(updates);

        let breaking_updates = report
            .breaking_updates()
            .map(|(image, tag, _)| format!("{} -!> {}:{}", image, image.name, tag))
            .collect::<Vec<_>>();
        let compatible_updates = report
            .compatible_updates()
            .map(|(image, tag, _)| format!("{} -> {}:{}", image, image.name, tag))
            .collect::<Vec<_>>();
        let no_updates = report
            .no_updates()
            .map(|(image, _)| image.to_string())
            .collect::<Vec<_>>();

        let failures = report
            .failures
            .into_iter()
            .map(|(image, error)| format!("`{}`: {:#}", image, anyhow::Error::new(error)))
            .collect::<Vec<_>>();

        if !failures.is_empty() {
            exit_code = EXIT_ERROR;
            eprintln!("{} failures:\n{}\n", failures.len(), failures.join("\n"));
        }
        if !breaking_updates.is_empty() {
            exit_code = EXIT_BREAKING_UPDATE;
            println!(
                "{} breaking updates:\n{}\n",
                breaking_updates.len(),
                breaking_updates.join("\n")
            );
        }
        if !compatible_updates.is_empty() {
            exit_code = EXIT_COMPATIBLE_UPDATE;
            println!(
                "{} compatible updates:\n{}\n",
                compatible_updates.len(),
                compatible_updates.join("\n")
            );
        }
        if !no_updates.is_empty() {
            println!(
                "{} have no updates in the latest {} tags:\n{}\n",
                no_updates.len(),
                amount,
                no_updates.join("\n")
            )
        }
    }

    Ok(exit_code)
}

fn check_compose(opts: CheckComposeOpts) -> Result<ExitCode> {
    let compose_file = fs::File::open(&opts.file)
        .with_context(|| format!("Failed to read file `{}`", opts.file.display()))?;
    let compose: DockerCompose =
        serde_yaml::from_reader(compose_file).context("Failed to parse Docker Compose file")?;

    let compose_dir = opts.file.parent().unwrap();
    let amount = 25;
    let updock = Updock::default();
    let services = compose.services.into_iter().map(|(service_name, service)| {
        let path = compose_dir.join(service.build).join("Dockerfile");
        let updates_result = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file `{}`", path.display()))
            .map(|input| updock.check_input(&input, amount).collect::<Vec<_>>());

        (service_name, updates_result)
    });

    let mut exit_code = EXIT_NO_UPDATE;

    if opts.check_opts.json {
        let report = DockerComposeReport::from(services);
        let successes = report
            .successes
            .into_iter()
            .map(|(service, updates)| {
                (
                    service,
                    updates
                        .into_iter()
                        .map(|(image, (update, _))| (image.to_string(), update))
                        .collect::<IndexMap<_, _>>(),
                )
            })
            .collect::<IndexMap<_, _>>();
        let failures = report
            .failures
            .into_iter()
            .map(|(service, result)| {
                (
                    service,
                    result
                        .map_err(|error| format!("{:#}", error))
                        .map(|updates| {
                            updates
                                .into_iter()
                                .map(|(_, error)| format!("{:#}", error))
                                .collect::<Vec<_>>()
                        }),
                )
            })
            .collect::<IndexMap<_, _>>();
        if !failures.is_empty() {
            exit_code.merge(&EXIT_ERROR);
        }

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "successes": successes,
                "failures": failures
            }))
            .context("Failed to serialize result")?
        );
    } else {
        for (service, result) in services {
            match result {
                Ok(updates) => {
                    println!("Service `{}`:", service);
                    for update_result in updates {
                        let (result, new_exit_code) = display_update(update_result, amount);
                        exit_code.merge(&new_exit_code);
                        match result {
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

    Ok(exit_code)
}

fn display_update(
    (image, update_result): DockerfileResult<DockerHubTagFetcher>,
    amount: usize,
) -> (Result<String, String>, ExitCode) {
    let mut exit_code = EXIT_NO_UPDATE;
    let result = update_result
        .map_err(|error| {
            exit_code.merge(&EXIT_ERROR);
            format!("Failed to check `{}`: {:#}", image, error)
        })
        .map(|(maybe_update, pattern_info)| {
            exit_code.merge(&ExitCode::from(&maybe_update));

            match maybe_update {
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
            }
        });

    (result, exit_code)
}
