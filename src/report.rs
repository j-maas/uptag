#[derive(Debug)]
pub struct Report<NoUpdate, Update, Error> {
    pub no_updates: Vec<NoUpdate>,
    pub compatible_updates: Vec<Update>,
    pub breaking_updates: Vec<Update>,
    pub failures: Vec<Error>,
}

impl<N, U, E> Report<N, U, E> {
    pub fn update_level(&self) -> UpdateLevel {
        use UpdateLevel::*;

        if !self.failures.is_empty() {
            Failure
        } else if !self.breaking_updates.is_empty() {
            BreakingUpdate
        } else if !self.compatible_updates.is_empty() {
            CompatibleUpdate
        } else {
            NoUpdates
        }
    }
}

pub enum UpdateLevel {
    NoUpdates,
    CompatibleUpdate,
    BreakingUpdate,
    Failure,
}

pub mod dockerfile {
    use super::*;

    use itertools::{Either, Itertools};

    use crate::{display_error, image::Image, Tag, Update};

    #[derive(Debug)]
    pub struct DockerfileReport<E>
    where
        E: 'static + std::error::Error,
    {
        pub report: Report<Image, (Image, Tag), (Image, E)>,
    }

    pub fn format_update(
        current_image: &Image,
        version_prefix: &'static str,
        new_tag: &str,
    ) -> String {
        let image_name = current_image.name.to_string();

        let prefix_width = std::cmp::max(version_prefix.len(), image_name.len());
        format!(
            "{image_name:>width$}:{current_tag}\n{version_prefix:>width$} {new_tag}",
            image_name = image_name,
            current_tag = current_image.tag,
            version_prefix = version_prefix,
            new_tag = new_tag,
            width = prefix_width
        )
    }

    impl<E> DockerfileReport<E>
    where
        E: 'static + std::error::Error,
    {
        pub fn from(results: impl Iterator<Item = (Image, Result<Update, E>)>) -> Self {
            let (successes, failures): (Vec<_>, Vec<_>) =
                results.partition_map(|(image, result)| match result {
                    Ok(info) => Either::Left((image, info)),
                    Err(error) => Either::Right((image, error)),
                });

            let mut no_updates = Vec::new();
            let mut compatible_updates = Vec::new();
            let mut breaking_updates = Vec::new();

            for (image, update) in successes {
                match update {
                    Update {
                        breaking: None,
                        compatible: None,
                    } => no_updates.push(image),
                    Update {
                        breaking: None,
                        compatible: Some(tag),
                    } => {
                        compatible_updates.push((image, tag));
                    }
                    Update {
                        breaking: Some(tag),
                        compatible: None,
                    } => {
                        breaking_updates.push((image, tag));
                    }
                    Update {
                        breaking: Some(breaking),
                        compatible: Some(compatible),
                    } => {
                        compatible_updates.push((image.clone(), compatible));
                        breaking_updates.push((image, breaking));
                    }
                }
            }

            DockerfileReport {
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
                .map(|(image, tag)| format_update(image, "-!>", tag))
                .collect::<Vec<_>>();
            let compatible_updates = self
                .report
                .compatible_updates
                .iter()
                .map(|(image, tag)| format_update(image, "->", tag))
                .collect::<Vec<_>>();
            let no_updates = self
                .report
                .no_updates
                .iter()
                .map(|image| image.to_string())
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

        pub fn display_failures(&self) -> String {
            let failures = self
                .report
                .failures
                .iter()
                .map(|(image, error)| format!("{}: {}", image, display_error(error)))
                .collect::<Vec<_>>();

            format!("{} with failure:\n{}", failures.len(), failures.join("\n"))
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        use crate::{dockerfile::CheckError, image::ImageName};

        type TestDockerfileResults = Vec<(Image, Result<Update, CheckError>)>;

        #[test]
        fn generates_dockerfile_report() {
            let success_image = Image {
                name: ImageName::new(None, "ubuntu".to_string()),
                tag: "14.04".to_string(),
            };
            let success_tag = "14.05".to_string();
            let success_update = Update {
                breaking: None,
                compatible: Some(success_tag.clone()),
            };

            let fail_image = Image {
                name: ImageName::new(None, "error".to_string()),
                tag: "1".to_string(),
            };
            let fail_error = CheckError::UnspecifiedPattern;

            let input: TestDockerfileResults = vec![
                (success_image.clone(), Ok(success_update)),
                (fail_image.clone(), Err(fail_error)),
            ];

            let result = DockerfileReport::from(input.into_iter());
            assert_eq!(
                result
                    .report
                    .compatible_updates
                    .into_iter()
                    .collect::<Vec<_>>(),
                vec![(success_image, success_tag)],
            );
            assert_eq!(
                result
                    .report
                    .failures
                    .into_iter()
                    .map(|(image, _)| image)
                    .collect::<Vec<_>>(),
                vec![fail_image]
            );
        }
    }
}

pub mod docker_compose {
    use super::*;

    use itertools::Itertools;

    use crate::{
        display_error,
        docker_compose::{ServiceName, Tag},
        image::Image,
        Update,
    };
    use dockerfile::{format_update, DockerfileReport};

    // Trait alias
    pub struct DockerComposeReport<E> {
        #[allow(clippy::type_complexity)]
        pub report: Report<
            (ServiceName, PathDisplay, Vec<Image>),
            (ServiceName, PathDisplay, Vec<(Image, Tag)>),
            (ServiceName, PathDisplay, DockerComposeResult<E>),
        >,
    }

    type PathDisplay = String;

    type DockerComposeResult<E> = Result<Vec<(Image, E)>, E>;

    impl<E> DockerComposeReport<E>
    where
        E: 'static + std::error::Error,
    {
        pub fn from(
            results: impl Iterator<
                Item = (
                    ServiceName,
                    PathDisplay,
                    Result<impl IntoIterator<Item = (Image, Result<Update, E>)>, E>,
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
                            no_updates.push((
                                service.clone(),
                                service_path.clone(),
                                report.no_updates,
                            ));
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
                    format!(
                        "{}\n{}",
                        display_service(service, service_path),
                        display_updates("-!>", updates.iter()),
                    )
                })
                .collect::<Vec<_>>();
            let compatible_updates = self
                .report
                .compatible_updates
                .iter()
                .map(|(service, service_path, updates)| {
                    format!(
                        "{}\n{}",
                        display_service(service, service_path),
                        display_updates("->", updates.iter()),
                    )
                })
                .collect::<Vec<_>>();
            let no_updates = self
                .report
                .no_updates
                .iter()
                .map(|(service, service_path, images)| {
                    format!(
                        "{}\n{}",
                        display_service(service, service_path),
                        display_images(images.iter()),
                    )
                })
                .collect::<Vec<_>>();

            let mut output = Vec::new();

            if !breaking_updates.is_empty() {
                output.push(format!(
                    "{} with breaking update:\n{}",
                    breaking_updates.len(),
                    breaking_updates.join("\n\n")
                ));
            }
            if !compatible_updates.is_empty() {
                output.push(format!(
                    "{} with compatible update:\n{}",
                    compatible_updates.len(),
                    compatible_updates.join("\n\n")
                ));
            }
            if !no_updates.is_empty() {
                output.push(format!(
                    "{} with no updates:\n{}",
                    no_updates.len(),
                    no_updates.join("\n\n")
                ));
            }

            output.join("\n\n\n")
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
                                format!("{}: {}", display_image(image), display_error(check_error))
                            })
                            .join("\n");
                        format!("{}\n{}", display_service(service, service_path), errors)
                    }
                    Err(error) => format!("  {}:\n  - {}", service, custom_display_error(error)),
                })
                .collect::<Vec<_>>();

            format!(
                "{} with failure:\n{}",
                failures.len(),
                failures.join("\n\n")
            )
        }
    }

    fn display_service(service: &str, service_path: &str) -> String {
        format!("  {} (at `{}`):", service, service_path)
    }

    fn display_updates<'a>(
        version_prefix: &'static str,
        updates: impl Iterator<Item = &'a (Image, String)>,
    ) -> String {
        updates
            .map(|(image, update)| {
                let output = format_update(image, version_prefix, update);
                let indented_output = output.replace("\n", "\n    ");
                format!("  - {}", indented_output)
            })
            .join("\n")
    }

    fn display_images<'a>(images: impl Iterator<Item = &'a Image>) -> String {
        images.map(display_image).join("\n")
    }

    fn display_image(image: &Image) -> String {
        format!("  - {}", image)
    }

    #[cfg(test)]
    mod test {
        use super::*;

        use crate::dockerfile::CheckError;
        use crate::image::ImageName;
        use crate::Update;

        type TestDockerComposeResults = Vec<(
            ServiceName,
            PathDisplay,
            Result<Vec<(Image, Result<Update, CheckError>)>, CheckError>,
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
            let compatible_update = Update {
                breaking: None,
                compatible: Some(compatible_tag.clone()),
            };

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
            let breaking_update = Update {
                compatible: None,
                breaking: Some(breaking_tag.clone()),
            };

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
}
