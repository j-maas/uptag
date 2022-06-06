use std::fs;
use std::path::{self, PathBuf};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use itertools::Itertools;
use lazy_static::lazy_static;
use structopt::StructOpt;
use thiserror::Error;

use docker_compose::BuildContext;
use uptag::docker_compose;
use uptag::dockerfile;
use uptag::dockerfile::CheckError;
use uptag::image::ImageName;
use uptag::report::{
    docker_compose::DockerComposeReport, dockerfile::DockerfileReport, UpdateLevel,
};
use uptag::tag_fetcher::{DockerHubTagFetcher, TagFetcher};
use uptag::version::extractor::VersionExtractor;
use uptag::FindUpdateError;

/// Check Docker image tags for updates.
#[derive(Debug, StructOpt)]
#[structopt(after_help = "\
PATTERN SYNTAX:
Use `<>` to match a number. Everything else will be matched literally.
- `<>.<>.<>` will match `2.13.3` but not `2.13.3a`.
- `debian-<>-beta` will match `debian-10-beta` but not `debian-10`.

Specify which numbers indicate breaking changes using `<!>`. Uptag will report breaking changes separately from compatible changes.
- Given pattern `<!>.<>.<>` and the current tag `1.4.12`:
  - compatible updates: `1.6.12` and `1.4.13`
  - breaking updates: `2.4.12` and `3.5.13`")]
enum Opts {
    Fetch(Box<FetchOpts>),
    Check(CheckOpts),
    CheckCompose(CheckComposeOpts),
}

/// Lists the latest tags for an image from DockerHub.
#[derive(Debug, StructOpt)]
struct FetchOpts {
    /// The image name for which tags should be fetched.
    image: ImageName,
    /// A pattern to filter the tags with. Only matching tags will be output.
    #[structopt(short, long)]
    pattern: Option<VersionExtractor>,
    /// The maximum number of tags to output.
    #[structopt(short, long, default_value = "25")]
    amount: usize,
    /// If given a pattern, limits how many tags will be searched through looking for a match.
    ///
    /// If --amount is larger, this will be set to --amount.
    ///
    /// Example: `uptag fetch --amount 50 --search-limit 500 --pattern '<!>.<>' ubuntu` will stop after 50 matching tags or after looking through the latest 500 tags, whichever happens first.
    #[structopt(short, long, default_value = "100")]
    search_limit: usize,
}

/// Reports on update status for all images in a Dockerfile.
#[derive(Debug, StructOpt)]
#[structopt(after_help = r#"SPECIFYING PATTERNS:
Each `FROM` definition needs to be annotated with a pattern and declare a specific tag that matches that pattern. The pattern must be given as a comment in the line before each `FROM <image>:<tag>` definition in the following format:
# uptag --pattern "<pattern>"

Example `Dockerfile`:
```
# uptag --pattern "<!>.<>"
FROM ubuntu:18.04

# uptag --pattern "<!>.<>.<>-slim"
FROM node:14.5.0-slim
```"#)]
struct CheckOpts {
    /// The Dockerfile to check.
    #[structopt(parse(from_os_str))]
    file: PathBuf,
    /// Limits how many tags will be fetched from DockerHub before stopping the search.
    #[structopt(short, long, default_value = "100")]
    search_limit: usize,
}

/// Reports on update status for all services in a docker-compose file.
#[derive(Debug, StructOpt)]
#[structopt(after_help = r#"SPECIFYING PATTERNS:
Each service must associate a pattern with its images. There are two supported declarations.

A service can specify an `image` field, pointing to an image on DockerHub. Such an image needs to be annotated with a pattern and declare a specific tag that matches that pattern. The pattern must be given as a comment in the line before the `image` field in the following format:
# uptag --pattern "<pattern>"

Alternatively, a service can point to a folder containing a Dockerfile via its `build` field. That Dockerfile needs to specify patterns as documented in `uptag check --help`.

Example `docker-compose.yml`:
```
version: "3.6"

services:
  ubuntu: 
    # uptag --pattern "<!>.<>"
    image: ubuntu:18.04

  alpine:
    build: ./alpine
```"#)]
struct CheckComposeOpts {
    /// The docker-compose file to check.
    #[structopt(parse(from_os_str))]
    file: PathBuf,
    /// Limits how many tags will be fetched from DockerHub before stopping the search.
    #[structopt(short, long, default_value = "100")]
    search_limit: usize,
}

fn main() {
    env_logger::init();

    let opts = Opts::from_args();

    use Opts::*;
    let result = match opts {
        Fetch(opts) => fetch(*opts),
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

    fn exit(&self) -> ! {
        std::process::exit(self.0)
    }
}

fn fetch(opts: FetchOpts) -> Result<ExitCode> {
    let adjusted_search_limit = std::cmp::max(opts.search_limit, opts.amount);
    let fetcher = DockerHubTagFetcher::with_search_limit(adjusted_search_limit);
    let tags = fetcher.fetch(&opts.image);

    let result = if let Some(extractor) = opts.pattern {
        let mut tag_count = 0;
        let result: Vec<String> = tags
            .filter_map(|tag_result| {
                tag_count += 1;
                tag_result
                    .map(|tag| {
                        if extractor.matches(&tag) {
                            Some(tag)
                        } else {
                            None
                        }
                    })
                    .transpose()
            })
            .take(opts.amount)
            .collect::<Result<_, _>>()
            .context("Failed to fetch tags")?;
        println!(
            "Fetched {} tags. Found {} matching `{}`:",
            tag_count,
            result.len(),
            extractor.pattern()
        );
        result
    } else {
        let fetched = tags
            .take(opts.amount)
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to fetch tags")?;
        println!("Fetched {} tags:", fetched.len());
        fetched
    };

    println!("{}", result.join("\n"));

    Ok(EXIT_OK)
}

fn check(opts: CheckOpts) -> Result<ExitCode> {
    let file_path = opts
        .file
        .canonicalize()
        .with_context(|| format!("Failed to find file `{}`", clean_path(&opts.file)))?;
    let input = fs::read_to_string(&file_path).with_context(|| {
        format!(
            "Failed to read file `{}`",
            display_canonicalized(&file_path)
        )
    })?;

    let fetcher = DockerHubTagFetcher::with_search_limit(opts.search_limit);
    let images = dockerfile::parse(&input);
    let updates = images.map(|(image, pattern_result)| {
        let results = pattern_result
            .map_err(UpdateError::Check)
            .and_then(|pattern| {
                let extractor = VersionExtractor::new(pattern);

                uptag::find_update(&fetcher, &image, &extractor).map_err(UpdateError::FindUpdate)
            });
        (image, results)
    });

    let dockerfile_report = DockerfileReport::from(updates);
    let exit_code = ExitCode::from(dockerfile_report.report.update_level());

    println!(
        "Report for Dockerfile at `{}`:\n",
        display_canonicalized(&file_path)
    );
    if !dockerfile_report.report.failures.is_empty() {
        eprintln!("{}", dockerfile_report.display_failures());
        println!();
    }
    println!("{}", dockerfile_report.display_successes());

    Ok(exit_code)
}

#[derive(Debug, Error)]
enum UpdateError<E>
where
    E: 'static + std::error::Error,
{
    #[error(transparent)]
    Check(#[from] CheckError),
    #[error(transparent)]
    FindUpdate(#[from] FindUpdateError<E>),
    #[error("Failed to find file `{file}`")]
    IO {
        file: String,
        #[source]
        source: std::io::Error,
    },
}

fn check_compose(opts: CheckComposeOpts) -> Result<ExitCode> {
    let compose_file_path = opts
        .file
        .canonicalize()
        .with_context(|| format!("Failed to find file `{}`", clean_path(&opts.file)))?;
    let compose_file = std::fs::read_to_string(&compose_file_path).with_context(|| {
        format!(
            "Failed to read file `{}`",
            display_canonicalized(&compose_file_path)
        )
    })?;
    let services =
        docker_compose::parse(&compose_file).context("Failed to parse docker-compose file")?;

    let compose_dir = opts.file.parent().unwrap();
    let fetcher = DockerHubTagFetcher::with_search_limit(opts.search_limit);

    let progress_bar = ProgressBar::new(services.len() as u64)
        .with_style(ProgressStyle::default_bar().template("{msg}\n{wide_bar} {pos}/{len}"));

    let updates = services.into_iter().map(|(service_name, build_context)| {
        progress_bar.set_message(format!(
            "Fetching for service `{service}`",
            service = service_name
        ));
        progress_bar.inc(1);

        match build_context {
            docker_compose::BuildContext::Image(image, pattern) => {
                let extractor = VersionExtractor::new(pattern);
                let update = uptag::find_update(&fetcher, &image, &extractor)
                    .map_err(UpdateError::FindUpdate);
                (service_name, BuildContext::Image(image, update))
            }
            docker_compose::BuildContext::Folder(relative_path, ()) => {
                let path = compose_dir.join(relative_path).join("Dockerfile");
                let path_display = path
                    .canonicalize()
                    .map(|path| display_canonicalized(&path))
                    .unwrap_or_else(|_| clean_path(&path));

                let updates_result = fs::read_to_string(&path)
                    .map_err(|error| UpdateError::IO {
                        file: clean_path(&path),
                        source: error,
                    })
                    .map(|input| {
                        let images = dockerfile::parse(&input);
                        let updates = images.map(|(image, pattern_result)| {
                            let results =
                                pattern_result
                                    .map_err(UpdateError::Check)
                                    .and_then(|pattern| {
                                        let extractor = VersionExtractor::new(pattern);

                                        uptag::find_update(&fetcher, &image, &extractor)
                                            .map_err(UpdateError::FindUpdate)
                                    });
                            (image, results)
                        });
                        updates.collect::<Vec<_>>()
                    });

                (
                    service_name,
                    BuildContext::Folder(path_display, updates_result),
                )
            }
        }
    });

    let docker_compose_report = DockerComposeReport::from(updates);

    progress_bar.finish_and_clear();

    let exit_code = ExitCode::from(docker_compose_report.report.update_level());

    println!(
        "Report for docker-compose file at `{}`:\n",
        display_canonicalized(&compose_file_path)
    );
    if !docker_compose_report.report.failures.is_empty() {
        eprintln!("{}", docker_compose_report.display_failures());
        println!("\n");
    }
    println!("{}", docker_compose_report.display_successes());

    Ok(exit_code)
}

/// Generates a String that displays the path more prettily than `path.display()`.
///
/// Assumes that the path is canonicalized.
fn display_canonicalized(path: &std::path::Path) -> String {
    if cfg!(not(target_os = "windows")) {
        path.display().to_string()
    } else {
        let mut output = path.display().to_string();
        // Removes the extended-length prefix.
        // See https://github.com/rust-lang/rust/issues/42869 for details.
        output.replace_range(..4, "");

        output
    }
}

lazy_static! {
    static ref SEPARATOR: String = std::path::MAIN_SEPARATOR.to_string();
    static ref CWD: PathBuf = std::env::current_dir().unwrap_or_default();
}

fn clean_path(path: &path::Path) -> String {
    let absolute_path = CWD.join(path);
    let mut components = absolute_path.components();

    fn component_to_string(c: path::Component) -> String {
        c.as_os_str().to_string_lossy().to_string()
    }
    let first = match components.next() {
        Some(path::Component::RootDir) => "".to_string(),
        Some(c) => component_to_string(c),
        None => return "".to_string(),
    };
    vec![first]
        .into_iter()
        .chain(components.filter_map(|c| match c {
            // Filter out all non-leading root-dirs to prevent surrounding them with extra separators.
            path::Component::RootDir => None,
            c => Some(component_to_string(c)),
        }))
        .join(&SEPARATOR)
}
