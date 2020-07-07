use std::path::PathBuf;

use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

use crate::{
    image::{Image, ImageName},
    pattern::{self, Pattern},
};

pub type ServiceName = String;

#[derive(Debug, PartialEq, Eq)]
pub enum BuildContext<I, P, F> {
    Image(Image, I),
    Folder(P, F),
}

pub fn parse(input: &str) -> Result<Vec<(ServiceName, BuildContext<Pattern, PathBuf, ()>)>, Error> {
    use Error::*;
    let parsed = marked_yaml::parse_yaml(0, input)?;
    let root = parsed.as_mapping().unwrap(); // root is always a mapping
    root.get_mapping("services")
        .ok_or_else(|| {
            if root.contains_key("services") {
                MalformedDockerfile()
            } else {
                MissingField("services")
            }
        })?
        .iter()
        .map(|(key, node)| {
            let service_name = key.as_str();
            let service = node.as_mapping().ok_or(MalformedDockerfile())?;
            let build_context = if let Some(path_node) = service.get_scalar("build") {
                let raw_path = path_node.as_str();
                BuildContext::Folder(raw_path.into(), ())
            } else if let Some(image_node) = service.get_scalar("image") {
                let raw_image = image_node.as_str();
                let captures = IMAGE
                    .captures(raw_image)
                    .ok_or_else(|| InvalidImage(raw_image.to_string()))?;
                let image_name = ImageName::new(
                    captures.name("user").map(|c| c.as_str().to_string()),
                    captures.name("image").unwrap().as_str().to_string(),
                );
                let tag = captures
                    .name("tag")
                    .map(|tag| tag.as_str())
                    .unwrap_or("latest");
                let image = Image {
                    name: image_name,
                    tag: tag.to_string(),
                };
                let image_line_number = image_node.span().start().unwrap().line();
                let (_, preceding_line) = input
                    .lines()
                    .enumerate()
                    .find(|(line_index, _)| *line_index == image_line_number - 2) // `line_index` starts at 0, `image_line_number` starts at 1.
                    .unwrap(); // We are guaranteed to have at least the `service` line before this line.
                let captures = PATTERN
                    .captures(preceding_line)
                    .ok_or_else(|| Error::MissingPattern(service_name.to_string()))?;
                let raw_pattern = captures.name("pattern").unwrap().as_str(); // Group `pattern` is required for the regex to match.
                let pattern =
                    Pattern::parse(raw_pattern).map_err(|error| Error::InvalidPattern {
                        service: service_name.to_string(),
                        pattern: raw_pattern.to_string(),
                        source: error,
                    })?;
                BuildContext::Image(image, pattern)
            } else {
                return Err(UnsupportedBuildContext {
                    service: service_name.to_string(),
                });
            };
            Ok((service_name.to_string(), build_context))
        })
        .collect()
}

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    #[error("Failed to read the input")]
    LoadError(#[from] marked_yaml::LoadError),
    #[error("The Dockerfile seems to be invalid")]
    MalformedDockerfile(),
    #[error("Failed to find `{0}`")]
    MissingField(&'static str),
    #[error("The image definition `{0}` is invalid")]
    InvalidImage(String),
    #[error("No build context was found for service `{service}` (Only the `build` and `image` fields containing strings are supported)")]
    UnsupportedBuildContext { service: String },
    #[error("Failed to find pattern for service `{0}` in the line before the `image` field")]
    MissingPattern(String),
    #[error("The pattern `{pattern}` for service `{service}` is invalid")]
    InvalidPattern {
        service: String,
        pattern: String,
        #[source]
        source: pattern::Error,
    },
}

lazy_static! {
    static ref IMAGE: Regex = Regex::new(
        r#"((?P<user>[[:word:]-]+)/)?(?P<image>[[:word:]-]+):(?P<tag>[[:word:][:punct:]]+)"#
    )
    .unwrap();
    static ref PATTERN: Regex =
        Regex::new(r#"#\s*uptag\s+--pattern\s+"(?P<pattern>[^"]*)""#).unwrap();
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_services() {
        let input = r#"
services:
    ubuntu:
        # uptag --pattern "<!>.<>"
        image: ubuntu:18.04
    
    alpine:
        build: ./alpine
        "#;
        assert_eq!(
            parse(input),
            Ok(vec![
                (
                    "ubuntu".to_string(),
                    BuildContext::Image(
                        Image {
                            name: ImageName::new(None, "ubuntu".to_string()),
                            tag: "18.04".to_string()
                        },
                        Pattern::parse("<!>.<>").unwrap()
                    )
                ),
                (
                    "alpine".to_string(),
                    BuildContext::Folder("./alpine".into(), ())
                )
            ])
        )
    }

    #[test]
    fn fails_when_services_is_missing() {
        let input = r#"
no: services
                "#;
        assert_eq!(parse(input), Err(Error::MissingField("services")))
    }

    #[test]
    fn fails_on_invalid_dockerfile() {
        let input = r#"
services: 
    - ubuntu
    - alpine:
                "#;
        assert_eq!(parse(input), Err(Error::MalformedDockerfile()))
    }

    #[test]
    fn fails_when_image_definition_is_invalid() {
        let input = r#"
services:
    ubuntu:
        image: "invalid/image/definition"
        "#;
        assert_eq!(
            parse(input),
            Err(Error::InvalidImage("invalid/image/definition".to_string()))
        )
    }

    #[test]
    fn fails_on_unsupported_build_context() {
        let input = r#"
services:
    alpine:
        build:
            context: unsupported
        "#;
        assert_eq!(
            parse(input),
            Err(Error::UnsupportedBuildContext {
                service: "alpine".to_string()
            })
        )
    }
}
