use crate::Update;

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

type UpdateResult<E> = Result<Update, E>;

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
        pub fn from(results: impl Iterator<Item = (Image, UpdateResult<E>)>) -> Self {
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
                    "{} breaking update(s):\n{}",
                    breaking_updates.len(),
                    breaking_updates.join("\n")
                ));
            }
            if !compatible_updates.is_empty() {
                output.push(format!(
                    "{} compatible update(s):\n{}",
                    compatible_updates.len(),
                    compatible_updates.join("\n")
                ));
            }
            if !no_updates.is_empty() {
                output.push(format!(
                    "{} without updates:\n{}",
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

            format!("{} failure(s):\n{}", failures.len(), failures.join("\n"))
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

    use super::dockerfile::{format_update, DockerfileReport};
    use crate::{
        display_error,
        docker_compose::{BuildContext, ServiceName},
        image::Image,
        Tag,
    };

    // Trait alias
    pub struct DockerComposeReport<E> {
        #[allow(clippy::type_complexity)]
        pub report: Report<
            (ServiceName, BuildContext<(), String, Vec<(Image, ())>>),
            (ServiceName, BuildContext<Tag, String, Vec<(Image, Tag)>>),
            (
                ServiceName,
                Result<BuildContext<E, String, Vec<(Image, E)>>, E>,
            ),
        >,
    }

    impl<E> DockerComposeReport<E>
    where
        E: 'static + std::error::Error,
    {
        pub fn from(
            results: impl Iterator<
                Item = (
                    ServiceName,
                    BuildContext<UpdateResult<E>, String, Result<Vec<(Image, UpdateResult<E>)>, E>>,
                ),
            >,
        ) -> Self {
            let mut no_updates = Vec::new();
            let mut compatible_updates = Vec::new();
            let mut breaking_updates = Vec::new();
            let mut failures = Vec::new();

            for (service, docker_compose_update) in results {
                match docker_compose_update {
                    BuildContext::Image(image, update_result) => match update_result {
                        Err(error) => {
                            failures.push((service.clone(), Ok(BuildContext::Image(image, error))))
                        }
                        Ok(update) => match update {
                            Update {
                                compatible: None,
                                breaking: None,
                            } => no_updates.push((service, BuildContext::Image(image, ()))),
                            Update {
                                compatible,
                                breaking,
                            } => {
                                if let Some(compatible_update) = compatible {
                                    compatible_updates.push((
                                        service.clone(),
                                        BuildContext::Image(image.clone(), compatible_update),
                                    ));
                                }
                                if let Some(breaking_udpate) = breaking {
                                    breaking_updates.push((
                                        service.clone(),
                                        BuildContext::Image(image.clone(), breaking_udpate),
                                    ));
                                }
                            }
                        },
                    },
                    BuildContext::Folder(path, result) => match result {
                        Ok(update_results) => {
                            let report = DockerfileReport::from(update_results.into_iter()).report;

                            if !report.no_updates.is_empty() {
                                let adapted_no_update = report
                                    .no_updates
                                    .into_iter()
                                    .map(|image| (image, ()))
                                    .collect();
                                no_updates.push((
                                    service.clone(),
                                    BuildContext::Folder(path.clone(), adapted_no_update),
                                ));
                            }
                            if !report.compatible_updates.is_empty() {
                                compatible_updates.push((
                                    service.clone(),
                                    BuildContext::Folder(path.clone(), report.compatible_updates),
                                ));
                            }
                            if !report.breaking_updates.is_empty() {
                                breaking_updates.push((
                                    service.clone(),
                                    BuildContext::Folder(path.clone(), report.breaking_updates),
                                ));
                            }
                            if !report.failures.is_empty() {
                                failures.push((
                                    service.clone(),
                                    Ok(BuildContext::Folder(path, report.failures)),
                                ));
                            }
                        }
                        Err(error) => failures.push((service, Err(error))),
                    },
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
                .map(|(service, build_context)| match build_context {
                    BuildContext::Image(image, update) => format!(
                        "{service}\n{updates}",
                        service = display_service_image(service, &image),
                        updates = display_update(&image, "-!>", update),
                    ),
                    BuildContext::Folder(service_path, updates) => format!(
                        "{service}\n{updates}",
                        service = display_service_folder(service, service_path),
                        updates = display_updates("-!>", updates.iter()),
                    ),
                })
                .collect::<Vec<_>>();
            let compatible_updates = self
                .report
                .compatible_updates
                .iter()
                .map(|(service, build_context)| match build_context {
                    BuildContext::Image(image, update) => format!(
                        "{service}\n{updates}",
                        service = display_service_image(service, &image),
                        updates = display_update(&image, "->", update),
                    ),
                    BuildContext::Folder(service_path, updates) => format!(
                        "{service}\n{updates}",
                        service = display_service_folder(service, service_path),
                        updates = display_updates("->", updates.iter()),
                    ),
                })
                .collect::<Vec<_>>();
            let no_updates = self
                .report
                .no_updates
                .iter()
                .map(|(service, build_context)| match build_context {
                    BuildContext::Image(image, ()) => display_service_image(service, &image),
                    BuildContext::Folder(service_path, images) => format!(
                        "{service}\n{images}",
                        service = display_service_folder(service, service_path),
                        images = display_images(images.iter().map(|(image, ())| image)),
                    ),
                })
                .collect::<Vec<_>>();

            let mut output = Vec::new();

            if !breaking_updates.is_empty() {
                output.push(format!(
                    "{} breaking update(s):\n{}",
                    breaking_updates.len(),
                    breaking_updates.join("\n\n")
                ));
            }
            if !compatible_updates.is_empty() {
                output.push(format!(
                    "{} compatible update(s):\n{}",
                    compatible_updates.len(),
                    compatible_updates.join("\n\n")
                ));
            }
            if !no_updates.is_empty() {
                output.push(format!(
                    "{} without updates:\n{}",
                    no_updates.len(),
                    no_updates.join("\n\n")
                ));
            }

            output.join("\n\n\n")
        }

        pub fn display_failures(&self) -> String {
            let failures = self
                .report
                .failures
                .iter()
                .map(|(service, build_context)| match build_context {
                    Err(error) => format!(
                        "  {service}: {error}",
                        service = service,
                        error = display_error(error)
                    ),
                    Ok(BuildContext::Image(image, error)) => format!(
                        "{service}\n{error}",
                        service = display_service_image(service, &image),
                        error = display_error(error)
                    ),
                    Ok(BuildContext::Folder(service_path, errors)) => {
                        let errors = errors
                            .iter()
                            .map(|(image, check_error)| {
                                format!(
                                    "{image}: {error}",
                                    image = display_image(image),
                                    error = display_error(check_error)
                                )
                            })
                            .join("\n");
                        format!(
                            "{service}\n{errors}",
                            service = display_service_folder(service, service_path),
                            errors = errors
                        )
                    }
                })
                .collect::<Vec<_>>();

            format!("{} failure(s):\n{}", failures.len(), failures.join("\n\n"))
        }
    }

    fn display_service_image(service: &str, image: &Image) -> String {
        format!(
            "  service `{service}` with image `{image}`:",
            service = service,
            image = image
        )
    }

    fn display_service_folder(service: &str, service_path: &str) -> String {
        format!(
            "  service `{service}` with Dockerfile at `{dockerfile_path}`:",
            service = service,
            dockerfile_path = service_path
        )
    }

    fn display_updates<'a>(
        version_prefix: &'static str,
        updates: impl Iterator<Item = &'a (Image, String)>,
    ) -> String {
        updates
            .map(|(image, update)| display_update(image, version_prefix, update))
            .join("\n")
    }

    fn display_update(image: &Image, version_prefix: &'static str, update: &str) -> String {
        let output = format_update(image, version_prefix, update);
        let indented_output = output.replace("\n", "\n    ");
        format!("  - {}", indented_output)
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
            let fail_error_copy = CheckError::UnspecifiedPattern;

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

            let fail_service = "debian".to_string();
            let fail_service_path = "path/to/debian".to_string();
            let fail_service_error = CheckError::UnspecifiedPattern; // This is not a realistic error. It could be an IO error when reading the path to the Dockerfile. But I was too lazy to introduce a common error type to hold both `CheckError`s and IO errors.
            let fail_service_error_copy = CheckError::UnspecifiedPattern;

            let node_service = "node".to_string();
            let node_image = Image {
                name: ImageName::new(None, "node".to_string()),
                tag: "14.4.0".to_string(),
            };
            let node_compatible_tag = "14.5.0".to_string();
            let node_compatible_update = Update {
                compatible: Some(node_compatible_tag.clone()),
                breaking: None,
            };

            let image_fail_service = "python".to_string();
            let image_fail_image = Image {
                name: ImageName::new(None, "python".to_string()),
                tag: "3.8.3".to_string(),
            };
            let image_fail_error = CheckError::UnspecifiedPattern;
            let image_fail_error_copy = CheckError::UnspecifiedPattern;

            let input = vec![
                (
                    ubuntu_service.clone(),
                    BuildContext::Folder(
                        ubuntu_path.clone(),
                        Ok(vec![
                            (compatible_image.clone(), Ok(compatible_update)),
                            (fail_image.clone(), Err(fail_error)),
                        ]),
                    ),
                ),
                (
                    alpine_service.clone(),
                    BuildContext::Folder(
                        alpine_path.clone(),
                        Ok(vec![(breaking_image.clone(), Ok(breaking_update))]),
                    ),
                ),
                (
                    fail_service.clone(),
                    BuildContext::Folder(fail_service_path, Err(fail_service_error)),
                ),
                (
                    node_service.clone(),
                    BuildContext::Image(node_image.clone(), Ok(node_compatible_update)),
                ),
                (
                    image_fail_service.clone(),
                    BuildContext::Image(image_fail_image.clone(), Err(image_fail_error)),
                ),
            ];

            let result = DockerComposeReport::from(input.into_iter());
            assert_eq!(
                result.report.compatible_updates,
                vec![
                    (
                        ubuntu_service.clone(),
                        BuildContext::Folder(
                            ubuntu_path.clone(),
                            vec![(compatible_image, compatible_tag)]
                        )
                    ),
                    (
                        node_service,
                        BuildContext::Image(node_image, node_compatible_tag)
                    )
                ]
            );
            assert_eq!(
                result.report.failures,
                vec![
                    (
                        ubuntu_service,
                        Ok(BuildContext::Folder(
                            ubuntu_path,
                            vec![(fail_image, fail_error_copy)]
                        ),)
                    ),
                    (fail_service, Err(fail_service_error_copy)),
                    (
                        image_fail_service,
                        Ok(BuildContext::Image(image_fail_image, image_fail_error_copy))
                    )
                ]
            );
            assert_eq!(
                result.report.breaking_updates,
                vec![(
                    alpine_service,
                    BuildContext::Folder(alpine_path, vec![(breaking_image, breaking_tag)])
                )]
            )
        }
    }
}
