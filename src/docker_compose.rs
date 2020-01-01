use std::path::PathBuf;

use indexmap::IndexMap;
use itertools::{Either, Itertools};
use serde::{Deserialize, Serialize};

use crate::image::Image;
use crate::tag_fetcher::TagFetcher;
use crate::{CheckError, DockerfileReport, DockerfileResult, PatternInfo, Update};

#[derive(Debug, Deserialize)]
pub struct DockerCompose {
    pub services: IndexMap<String, Service>, // IndexMap preserves order.
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Service {
    pub build: PathBuf,
}

type ServiceName = String;

// Trait alias
pub struct DockerComposeReport<T, E>
where
    T: std::fmt::Debug + TagFetcher,
    T::Error: 'static,
{
    pub successes: DockerComposeSuccesses,
    pub failures: DockerComposeFailures<T, E>,
}

type DockerComposeSuccesses = Vec<(ServiceName, Vec<(Image, (Option<Update>, PatternInfo))>)>;
type DockerComposeFailures<T, E> = Vec<(ServiceName, Result<Vec<(Image, CheckError<T>)>, E>)>;

impl<T, E> DockerComposeReport<T, E>
where
    T: std::fmt::Debug + TagFetcher,
    T::Error: 'static,
{
    pub fn from(
        results: impl Iterator<
            Item = (
                ServiceName,
                Result<impl IntoIterator<Item = DockerfileResult<T>>, E>,
            ),
        >,
    ) -> Self {
        let (successes, failures): (Vec<_>, Vec<_>) = results
            .flat_map(|(service, result)| match result {
                Ok(updates_result) => {
                    let report = DockerfileReport::from(updates_result.into_iter());

                    let mut result = vec![Either::Left((service.clone(), report.successes))];
                    if !report.failures.is_empty() {
                        result.push(Either::Right((service, Ok(report.failures))));
                    }

                    result
                }
                Err(error) => vec![Either::Right((service, Err(error)))],
            })
            .partition_map(|item| item);

        DockerComposeReport {
            successes,
            failures,
        }
    }
}
