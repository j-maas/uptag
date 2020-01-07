use std::path::PathBuf;

use indexmap::IndexMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::display_error;
use crate::dockerfile::{CheckError, DockerfileReport, DockerfileResult};
use crate::image::Image;
use crate::report::Report;
use crate::tag_fetcher::TagFetcher;

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
    #[allow(clippy::type_complexity)]
    pub report: Report<ServiceName, Vec<(Image, Tag)>, DockerComposeResult<T, E>, Vec<(Image, ())>>,
}

type DockerComposeResult<T, E> = Result<Vec<(Image, CheckError<T>)>, E>;

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
        let mut no_updates = Vec::new();
        let mut compatible_updates = Vec::new();
        let mut breaking_updates = Vec::new();
        let mut failures = Vec::new();

        for (service, result) in results {
            match result {
                Ok(updates_result) => {
                    let report = DockerfileReport::from(updates_result.into_iter()).report;

                    if !report.no_updates.is_empty() {
                        no_updates.push((service.clone(), report.no_updates));
                    }
                    if !report.compatible_updates.is_empty() {
                        compatible_updates.push((service.clone(), report.compatible_updates));
                    }
                    if !report.breaking_updates.is_empty() {
                        breaking_updates.push((service.clone(), report.breaking_updates));
                    }
                    if !report.failures.is_empty() {
                        failures.push((service.clone(), Ok(report.failures)));
                    }
                }
                Err(error) => {
                    failures.push((service, Err(error)));
                }
            }
        }

        DockerComposeReport {
            report: Report {
                no_updates,
                compatible_updates,
                breaking_updates,
                failures,
            },
        }
    }

    pub fn display_successes(&self) -> String {
        let breaking_updates = self
            .report
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
            .report
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
            .report
            .no_updates
            .iter()
            .map(|(service, images)| {
                let output = images
                    .iter()
                    .map(|(image, ())| format!("  {}", image))
                    .join("\n");
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
            .report
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

#[cfg(test)]
mod test {
    use super::*;

    use crate::image::ImageName;
    use crate::tag_fetcher::test::ArrayFetcher;
    use crate::tag_fetcher::CurrentTag;
    use crate::version_extractor::VersionExtractor;
    use crate::{PatternInfo, Update};

    type TestDockerComposeResults = Vec<(
        ServiceName,
        Result<
            Vec<(
                Image,
                Result<(Option<Update>, CurrentTag, PatternInfo), CheckError<ArrayFetcher>>,
            )>,
            (),
        >,
    )>;

    #[test]
    fn generates_docker_compose_report() {
        let ubuntu_service = "ubuntu".to_string();
        let compatible_image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let compatible_tag = "14.05".to_string();
        let compatible_update = (
            Some(Update::Compatible(compatible_tag.clone())),
            CurrentTag::Found,
            PatternInfo {
                extractor: VersionExtractor::parse("<>.<>.<>").unwrap(),
                breaking_degree: 1,
            },
        );

        let fail_image = Image {
            name: ImageName::new(None, "error".to_string()),
            tag: "1".to_string(),
        };
        let fail_error = CheckError::UnspecifiedPattern;

        let alpine_service = "alpine".to_string();
        let breaking_image = Image {
            name: ImageName::new(None, "alpine".to_string()),
            tag: "3.8.4".to_string(),
        };
        let breaking_tag = "4.0.2".to_string();
        let breaking_update = (
            Some(Update::Breaking(breaking_tag.clone())),
            CurrentTag::Found,
            PatternInfo {
                extractor: VersionExtractor::parse("<>.<>.<>").unwrap(),
                breaking_degree: 1,
            },
        );

        let input: TestDockerComposeResults = vec![
            (
                ubuntu_service.clone(),
                Ok(vec![
                    (compatible_image.clone(), Ok(compatible_update)),
                    (fail_image.clone(), Err(fail_error)),
                ]),
            ),
            (
                alpine_service.clone(),
                Ok(vec![(breaking_image.clone(), Ok(breaking_update))]),
            ),
        ];

        let result = DockerComposeReport::from(input.into_iter());
        assert_eq!(
            result.report.compatible_updates,
            vec![(
                ubuntu_service.clone(),
                vec![(compatible_image, compatible_tag)]
            )]
        );
        assert_eq!(
            result
                .report
                .failures
                .into_iter()
                .map(|(service, result)| {
                    (
                        service,
                        result.map(|images| {
                            images
                                .into_iter()
                                .map(|(image, _)| image)
                                .collect::<Vec<_>>()
                        }),
                    )
                })
                .collect::<Vec<_>>(),
            vec![(ubuntu_service, Ok(vec![fail_image]))]
        );
        assert_eq!(
            result.report.breaking_updates,
            vec![(alpine_service, vec![(breaking_image, breaking_tag)])]
        )
    }
}
