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

use updock::image::ImageName;
use updock::report::{
    DockerCompose, DockerComposeReport, Dockerfile, DockerfileReport, UpdateLevel,
};
use updock::tag_fetcher::{DockerHubTagFetcher, TagFetcher};
use updock::version_extractor::VersionExtractor;
use updock::Updock;

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

fn main() {
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
    fn from(level: UpdateLevel) -> ExitCode {
        use UpdateLevel::*;
        match level {
            Failure => EXIT_ERROR,
            BreakingUpdate => EXIT_BREAKING_UPDATE,
            CompatibleUpdate => EXIT_COMPATIBLE_UPDATE,
            NoUpdates => EXIT_NO_UPDATE,
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
        .fetch(&opts.image)
        .take(opts.amount)
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to fetch tags")?;

    let result = if let Some(extractor) = opts.pattern {
        let tag_count = tags.len();
        let result: Vec<String> = extractor.filter(tags).collect();
        println!(
            "Fetched {} tags. Found {} matching `{}`:",
            tag_count,
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

    let updock = Updock::default();
    let updates = Dockerfile::check_input(&updock, &input);

    let dockerfile_report = DockerfileReport::from(updates);
    let exit_code = ExitCode::from(dockerfile_report.report.update_level());

    if opts.json {
        let report = dockerfile_report.report;
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

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "failures": failures,
                "no_updates": report.no_updates,
                "compatible_updates": report.compatible_updates,
                "breaking_updates": report.breaking_updates
            }))
            .context("Failed to serialize result")?
        );
    } else {
        if !dockerfile_report.report.failures.is_empty() {
            eprintln!("{}", dockerfile_report.display_failures());
            println!();
        }
        println!("{}", dockerfile_report.display_successes());
    }

    Ok(exit_code)
}

fn check_compose(opts: CheckComposeOpts) -> Result<ExitCode> {
    let compose_file = fs::File::open(&opts.file)
        .with_context(|| format!("Failed to read file `{}`", opts.file.display()))?;
    let compose: DockerCompose =
        serde_yaml::from_reader(compose_file).context("Failed to parse Docker Compose file")?;

    let compose_dir = opts.file.parent().unwrap();
    let updock = Updock::default();
    let services = compose.services.into_iter().map(|(service_name, service)| {
        let path = compose_dir.join(service.build).join("Dockerfile");
        let updates_result = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file `{}`", path.display()))
            .map(|input| Dockerfile::check_input(&updock, &input).collect::<Vec<_>>());

        (service_name, updates_result)
    });

    let docker_compose_report = DockerComposeReport::from(services);

    let mut exit_code = ExitCode::from(docker_compose_report.report.update_level());

    if opts.check_opts.json {
        let report = docker_compose_report.report;
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
                                .map(|(image, error)| {
                                    (image, format!("{:#}", anyhow::Error::new(error)))
                                })
                                .collect::<IndexMap<_, _>>()
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
                "failures": failures,
                "no_updates": report.no_updates,
                "compatible_updates": report.compatible_updates,
                "breaking_updates": report.breaking_updates
            }))
            .context("Failed to serialize result")?
        );
    } else {
        if !docker_compose_report.report.failures.is_empty() {
            eprintln!(
                "{}",
                docker_compose_report.display_failures(|error| format!("{:#}", error))
            );
            println!();
        }
        println!("{}", docker_compose_report.display_successes());
    }

    Ok(exit_code)
}
