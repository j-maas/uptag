use std::path::PathBuf;

use indexmap::IndexMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::image::Image;
use crate::tag_fetcher::TagFetcher;
use crate::{display_error, CheckError, DockerfileReport, DockerfileResult};

#[derive(Debug, Deserialize)]
pub struct DockerCompose {
    pub services: IndexMap<String, Service>, // IndexMap preserves order.
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Service {
    pub build: PathBuf,
}

type ServiceName = String;
type Tag = String;

// Trait alias
pub struct DockerComposeReport<T, E>
where
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
    pub no_updates: IndexMap<ServiceName, Vec<Image>>,
    pub compatible_updates: IndexMap<ServiceName, IndexMap<Image, Tag>>,
    pub breaking_updates: IndexMap<ServiceName, IndexMap<Image, Tag>>,
    pub failures: DockerComposeFailures<T, E>,
}

type DockerComposeFailures<T, E> = IndexMap<ServiceName, Result<IndexMap<Image, CheckError<T>>, E>>;

impl<T, E> DockerComposeReport<T, E>
where
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
    pub fn from(
        results: impl Iterator<
            Item = (
                ServiceName,
                Result<impl IntoIterator<Item = DockerfileResult<T>>, E>,
            ),
        >,
    ) -> Self {
        let mut no_updates = IndexMap::new();
        let mut compatible_updates = IndexMap::new();
        let mut breaking_updates = IndexMap::new();
        let mut failures = IndexMap::new();

        for (service, result) in results {
            match result {
                Ok(updates_result) => {
                    let report = DockerfileReport::from(updates_result.into_iter());

                    if !report.no_updates.is_empty() {
                        no_updates.insert(service.clone(), report.no_updates);
                    }
                    if !report.compatible_updates.is_empty() {
                        compatible_updates.insert(service.clone(), report.compatible_updates);
                    }
                    if !report.breaking_updates.is_empty() {
                        breaking_updates.insert(service.clone(), report.breaking_updates);
                    }
                    if !report.failures.is_empty() {
                        failures.insert(service.clone(), Ok(report.failures));
                    }
                }
                Err(error) => {
                    failures.insert(service, Err(error));
                }
            }
        }

        DockerComposeReport {
            no_updates,
            compatible_updates,
            breaking_updates,
            failures,
        }
    }

    pub fn display_successes(&self) -> String {
        let breaking_updates = self
            .breaking_updates
            .iter()
            .map(|(service, updates)| {
                let output = updates
                    .iter()
                    .map(|(image, update)| format!("  {} -!> {}:{}", image, image.name, update))
                    .join("\n");
                format!("{}:\n{}", service, output)
            })
            .collect::<Vec<_>>();
        let compatible_updates = self
            .compatible_updates
            .iter()
            .map(|(service, updates)| {
                let output = updates
                    .iter()
                    .map(|(image, update)| format!("  {} -> {}:{}", image, image.name, update))
                    .join("\n");
                format!("{}:\n{}", service, output)
            })
            .collect::<Vec<_>>();
        let no_updates = self
            .no_updates
            .iter()
            .map(|(service, images)| {
                let output = images.iter().map(|image| format!("  {}", image)).join("\n");
                format!("{}:\n{}", service, output)
            })
            .collect::<Vec<_>>();

        let mut output = Vec::new();

        if !breaking_updates.is_empty() {
            output.push(format!(
                "{} with breaking update:\n{}",
                breaking_updates.len(),
                breaking_updates.join("\n")
            ));
        }
        if !compatible_updates.is_empty() {
            output.push(format!(
                "{} with compatible update:\n{}",
                compatible_updates.len(),
                compatible_updates.join("\n")
            ));
        }
        if !no_updates.is_empty() {
            output.push(format!(
                "{} with no updates:\n{}",
                no_updates.len(),
                no_updates.join("\n")
            ));
        }

        output.join("\n\n")
    }

    pub fn display_failures(&self, custom_display_error: impl Fn(&E) -> String) -> String {
        let failures = self
            .failures
            .iter()
            .map(|(service, error)| match error {
                Ok(check_errors) => {
                    let errors = check_errors
                        .iter()
                        .map(|(image, check_error)| {
                            format!("  {}: {}", image, display_error(check_error))
                        })
                        .join("\n");
                    format!("{}:\n{}", service, errors)
                }
                Err(error) => format!("{}: {}", service, custom_display_error(error)),
            })
            .collect::<Vec<_>>();

        format!("{} with failure:\n{}", failures.len(), failures.join("\n"))
    }
}
