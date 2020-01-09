use std::path::PathBuf;

use indexmap::IndexMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::display_error;
use crate::dockerfile::{DockerfileReport, DockerfileResult, ReportError};
use crate::image::Image;
use crate::report::Report;

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
    T: 'static + std::error::Error,
{
    #[allow(clippy::type_complexity)]
    pub report: Report<
        (ServiceName, PathDisplay, Vec<Image>),
        (ServiceName, PathDisplay, Vec<(Image, Tag)>),
        (ServiceName, PathDisplay, DockerComposeResult<T, E>),
    >,
}

type PathDisplay = String;

type DockerComposeResult<T, E> = Result<Vec<(Image, ReportError<T>)>, E>;

impl<T, E> DockerComposeReport<T, E>
where
    T: 'static + std::error::Error,
{
    pub fn from(
        results: impl Iterator<
            Item = (
                ServiceName,
                PathDisplay,
                Result<impl IntoIterator<Item = DockerfileResult<T>>, E>,
            ),
        >,
    ) -> Self {
        let mut no_updates = Vec::new();
        let mut compatible_updates = Vec::new();
        let mut breaking_updates = Vec::new();
        let mut failures = Vec::new();

        for (service, service_path, result) in results {
            match result {
                Ok(updates_result) => {
                    let report = DockerfileReport::from(updates_result.into_iter()).report;

                    if !report.no_updates.is_empty() {
                        no_updates.push((service.clone(), service_path.clone(), report.no_updates));
                    }
                    if !report.compatible_updates.is_empty() {
                        compatible_updates.push((
                            service.clone(),
                            service_path.clone(),
                            report.compatible_updates,
                        ));
                    }
                    if !report.breaking_updates.is_empty() {
                        breaking_updates.push((
                            service.clone(),
                            service_path.clone(),
                            report.breaking_updates,
                        ));
                    }
                    if !report.failures.is_empty() {
                        failures.push((service.clone(), service_path, Ok(report.failures)));
                    }
                }
                Err(error) => {
                    failures.push((service, service_path, Err(error)));
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
            .map(|(service, service_path, updates)| {
                let output = updates
                    .iter()
                    .map(|(image, update)| format!("  {} -!> {}:{}", image, image.name, update))
                    .join("\n");
                format!("{} (from file `{}`):\n{}", service, service_path, output)
            })
            .collect::<Vec<_>>();
        let compatible_updates = self
            .report
            .compatible_updates
            .iter()
            .map(|(service, service_path, updates)| {
                let output = updates
                    .iter()
                    .map(|(image, update)| format!("  {} -> {}:{}", image, image.name, update))
                    .join("\n");
                format!("{} (from file `{}`):\n{}", service, service_path, output)
            })
            .collect::<Vec<_>>();
        let no_updates = self
            .report
            .no_updates
            .iter()
            .map(|(service, service_path, images)| {
                let output = images.iter().map(|image| format!("  {}", image)).join("\n");
                format!("{} (from file `{}`):\n{}", service, service_path, output)
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
            .map(|(service, service_path, error)| match error {
                Ok(check_errors) => {
                    let errors = check_errors
                        .iter()
                        .map(|(image, check_error)| {
                            format!("  {}: {}", image, display_error(check_error))
                        })
                        .join("\n");
                    format!("{} (from file `{}`):\n{}", service, service_path, errors)
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

    use crate::dockerfile::CheckError;
    use crate::image::ImageName;
    use crate::tag_fetcher::test::FetchError;
    use crate::tag_fetcher::CurrentTag;
    use crate::version_extractor::VersionExtractor;
    use crate::Update;

    type TestDockerComposeResults = Vec<(
        ServiceName,
        PathDisplay,
        Result<
            Vec<(
                Image,
                Result<(Option<Update>, CurrentTag, VersionExtractor), CheckError<FetchError>>,
            )>,
            (),
        >,
    )>;

    #[test]
    fn generates_docker_compose_report() {
        let ubuntu_service = "ubuntu".to_string();
        let ubuntu_path = "/path/to/ubuntu".to_string();

        let compatible_image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let compatible_tag = "14.05".to_string();
        let compatible_update = (
            Some(Update::Compatible(compatible_tag.clone())),
            CurrentTag::Found,
            VersionExtractor::parse("<>.<>.<>").unwrap(),
        );

        let fail_image = Image {
            name: ImageName::new(None, "error".to_string()),
            tag: "1".to_string(),
        };
        let fail_error = CheckError::UnspecifiedPattern;

        let alpine_service = "alpine".to_string();
        let alpine_path = "path/to/alpine".to_string();

        let breaking_image = Image {
            name: ImageName::new(None, "alpine".to_string()),
            tag: "3.8.4".to_string(),
        };
        let breaking_tag = "4.0.2".to_string();
        let breaking_update = (
            Some(Update::Breaking(breaking_tag.clone())),
            CurrentTag::Found,
            VersionExtractor::parse("<>.<>.<>").unwrap(),
        );

        let input: TestDockerComposeResults = vec![
            (
                ubuntu_service.clone(),
                ubuntu_path.clone(),
                Ok(vec![
                    (compatible_image.clone(), Ok(compatible_update)),
                    (fail_image.clone(), Err(fail_error)),
                ]),
            ),
            (
                alpine_service.clone(),
                alpine_path.clone(),
                Ok(vec![(breaking_image.clone(), Ok(breaking_update))]),
            ),
        ];

        let result = DockerComposeReport::from(input.into_iter());
        assert_eq!(
            result.report.compatible_updates,
            vec![(
                ubuntu_service.clone(),
                ubuntu_path.clone(),
                vec![(compatible_image, compatible_tag)]
            )]
        );
        assert_eq!(
            result
                .report
                .failures
                .into_iter()
                .map(|(service, service_path, result)| {
                    (
                        service,
                        service_path,
                        result.map(|images| {
                            images
                                .into_iter()
                                .map(|(image, _)| image)
                                .collect::<Vec<_>>()
                        }),
                    )
                })
                .collect::<Vec<_>>(),
            vec![(ubuntu_service, ubuntu_path, Ok(vec![fail_image]))]
        );
        assert_eq!(
            result.report.breaking_updates,
            vec![(
                alpine_service,
                alpine_path,
                vec![(breaking_image, breaking_tag)]
            )]
        )
    }
}
