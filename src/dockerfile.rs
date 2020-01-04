use indexmap::IndexMap;
use itertools::{Either, Itertools};
use thiserror::Error;

use crate::image::Image;
use crate::matches::Matches;
use crate::tag_fetcher::{FetchUntilError, TagFetcher};
use crate::version_extractor;
use crate::version_extractor::VersionExtractor;
use crate::{display_error, PatternInfo, Update, Updock, Version};

pub struct Dockerfile {}

impl Dockerfile {
    pub fn check_input<'a, T>(
        updock: &'a Updock<T>,
        input: &'a str,
    ) -> impl DockerfileResults<T> + 'a
    where
        T: TagFetcher + std::fmt::Debug,
        T::FetchError: 'static,
    {
        Matches::iter(input).map(move |matches| {
            let image = matches.image();
            let result = Self::extract_check_info(
                &image.tag,
                &matches.pattern().map(|m| m.as_str()),
                matches.breaking_degree().unwrap_or(0),
            )
            .and_then(|(current_version, pattern_info)| {
                updock
                    .find_update(&image, &current_version, &pattern_info)
                    .map_err(CheckError::FailedFetch)
                    .map(|maybe_update| (maybe_update, pattern_info))
            });

            (image, result)
        })
    }

    fn extract_check_info<T>(
        tag: &str,
        pattern: &Option<&str>,
        breaking_degree: usize,
    ) -> Result<(Version, PatternInfo), CheckError<T>>
    where
        T: TagFetcher + std::fmt::Debug,
    {
        use CheckError::*;

        let pattern = pattern.ok_or(UnspecifiedPattern)?;
        let extractor = VersionExtractor::parse(pattern).map_err(|error| InvalidPattern {
            pattern: pattern.to_string(),
            source: error,
        })?;
        let current_version = extractor.extract_from(tag).ok_or(InvalidCurrentTag {
            tag: tag.to_string(),
            pattern: extractor.to_string(),
        })?;
        let breaking_degree = breaking_degree;
        Ok((
            current_version,
            PatternInfo {
                extractor,
                breaking_degree,
            },
        ))
    }
}

type Tag = String;

#[derive(Debug, Error)]
pub enum CheckError<T>
where
    // The Debug trait is required here, because the Debug derive incorrectly infers trait bounds on `T`.
    // For details, see https://github.com/rust-lang/rust/issues/26925
    // Including this bound is the easiest workaround, since TagFetchers can easily derive Debug.
    T: TagFetcher + std::fmt::Debug,
    T::FetchError: 'static,
{
    #[error("Failed to fetch tags")]
    FailedFetch(#[source] FetchUntilError<T::FetchError>),
    #[error("The current tag `{tag}` does not match the required pattern `{pattern}`")]
    InvalidCurrentTag { tag: Tag, pattern: String },
    #[error("Failed to find version pattern")]
    UnspecifiedPattern,
    #[error("The version pattern `{pattern}` is invalid")]
    InvalidPattern {
        pattern: String,
        #[source]
        source: version_extractor::Error,
    },
}

pub type DockerfileResult<T> = (Image, Result<(Option<Update>, PatternInfo), CheckError<T>>);

// Trait alias
pub trait DockerfileResults<T>: Iterator<Item = DockerfileResult<T>>
where
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
}

impl<A, T> DockerfileResults<T> for A
where
    A: Iterator<Item = DockerfileResult<T>>,
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
}

#[derive(Debug)]
pub struct DockerfileReport<T>
where
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
    pub no_updates: Vec<Image>,
    pub compatible_updates: IndexMap<Image, Tag>,
    pub breaking_updates: IndexMap<Image, Tag>,
    pub failures: IndexMap<Image, CheckError<T>>,
}

impl<T> DockerfileReport<T>
where
    T: std::fmt::Debug + TagFetcher,
    T::FetchError: 'static,
{
    pub fn from(results: impl DockerfileResults<T>) -> Self {
        let (successes, failures): (Vec<_>, IndexMap<_, _>) =
            results.partition_map(|(image, result)| match result {
                Ok(info) => Either::Left((image, info)),
                Err(error) => Either::Right((image, error)),
            });

        let mut no_updates = Vec::new();
        let mut compatible_updates = IndexMap::new();
        let mut breaking_updates = IndexMap::new();

        for (image, (maybe_update, _)) in successes {
            match maybe_update {
                None => no_updates.push(image),
                Some(Update::Compatible(tag)) => {
                    compatible_updates.insert(image, tag);
                }
                Some(Update::Breaking(tag)) => {
                    breaking_updates.insert(image, tag);
                }
                Some(Update::Both {
                    compatible,
                    breaking,
                }) => {
                    compatible_updates.insert(image.clone(), compatible);
                    breaking_updates.insert(image, breaking);
                }
            }
        }

        DockerfileReport {
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
            .map(|(image, tag)| format!("{} -!> {}:{}", image, image.name, tag))
            .collect::<Vec<_>>();
        let compatible_updates = self
            .compatible_updates
            .iter()
            .map(|(image, tag)| format!("{} -> {}:{}", image, image.name, tag))
            .collect::<Vec<_>>();
        let no_updates = self
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

    use crate::image::ImageName;
    use crate::tag_fetcher::test::ArrayFetcher;

    type TestDockerfileResults = Vec<(
        Image,
        Result<(Option<Update>, PatternInfo), CheckError<ArrayFetcher>>,
    )>;

    #[test]
    fn generates_dockerfile_report() {
        let success_image = Image {
            name: ImageName::new(None, "ubuntu".to_string()),
            tag: "14.04".to_string(),
        };
        let success_tag = "14.05".to_string();
        let success_update = (
            Some(Update::Compatible(success_tag.clone())),
            PatternInfo {
                extractor: VersionExtractor::parse("").unwrap(),
                breaking_degree: 1,
            },
        );

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
            result.compatible_updates.into_iter().collect::<Vec<_>>(),
            vec![(success_image, success_tag)],
        );
        assert_eq!(
            result
                .failures
                .into_iter()
                .map(|(image, _)| image)
                .collect::<Vec<_>>(),
            vec![fail_image]
        );
    }
}
