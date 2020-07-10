use std::fmt;

use lazy_static::lazy_static;
use regex::Regex;
use serde::{Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Image {
    pub name: ImageName,
    pub tag: Tag,
}

pub type Tag = String;

impl std::str::FromStr for Image {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let captures = IMAGE_REGEX.captures(s).ok_or(())?;
        let full_match = captures.get(0).unwrap(); // Group 0 is always the full match.
        if full_match.as_str().len() != s.len() {
            // The string contained extra character that do not belong in an image.
            return Err(());
        }
        let user = captures.name("user").map(|m| m.as_str().to_string());
        let image = captures.name("image").unwrap().as_str().to_string(); // An image is required for a match.
        let tag = captures
            .name("tag")
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "latest".to_string());
        Ok(Image {
            name: ImageName::new(user, image),
            tag,
        })
    }
}
lazy_static! {
    pub static ref IMAGE_REGEX: Regex = Regex::new(
        r#"((?P<user>[[:word:]-]+)/)?(?P<image>[[:word:]-]+)(:(?P<tag>[[:word:][:punct:]]+))?"#
    )
    .unwrap();
}

impl fmt::Display for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.name, self.tag)
    }
}

impl Serialize for Image {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum ImageName {
    Official { image: String },
    User { user: String, image: String },
}

// "Name components may contain lowercase letters, digits and separators.
// A separator is defined as a period, one or two underscores, or one or more dashes.
// A name component may not start or end with a separator."
// - https://docs.docker.com/engine/reference/commandline/tag/#extended-description
//
// We will not check whether these restrictions are violated, because that would
// make it unnecessarily complex. The consequence is that the image will not be found.
// We will, however, allow only the specified character set.
fn name_pattern() -> String {
    let name_characters = r"[a-z0-9._-]+";
    format!(
        r"((?P<first>{name_chars})/)?(?P<second>{name_chars})",
        name_chars = name_characters
    )
}
lazy_static! {
    static ref NAME: Regex = Regex::new(&format!("^{}$", name_pattern())).unwrap();
}

impl ImageName {
    pub fn new(user: Option<String>, image: String) -> ImageName {
        match user {
            Some(name) => ImageName::User { user: name, image },
            None => ImageName::Official { image },
        }
    }

    pub fn parse(image: &str) -> Option<ImageName> {
        NAME.captures(image).map(|captures| {
            let first = captures.name("first").map(|s| s.as_str().into());
            let second = captures["second"].into(); // Second group is not optional, so access is safe.
            match first {
                Some(user) => ImageName::User {
                    user,
                    image: second,
                },
                None => ImageName::Official { image: second },
            }
        })
    }
}

impl fmt::Display for ImageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ImageName::*;
        match self {
            Official { image } => write!(f, "{}", image),
            User { user, image } => write!(f, "{}/{}", user, image),
        }
    }
}

impl std::str::FromStr for ImageName {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(ParseError {
            invalid: s.to_string(),
        })
    }
}

#[derive(Debug, Error)]
#[error("`{invalid}` is not a valid name of the form `<image>` or `<user>/<image>`")]
pub struct ParseError {
    invalid: String,
}

#[cfg(test)]
mod test {
    use super::*;

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parses_valid_official_name(image in r"[a-z0-9]+[-_\.][a-z0-9]+") {
            let expected = ImageName::Official { image: image.clone()};
            prop_assert_eq!(ImageName::parse(&image), Some(expected));
        }

        #[test]
        fn parses_valid_user_name(first in r"[a-z0-9]+[-_\.][a-z0-9]+", second in r"[a-z0-9]+[-_\.][a-z0-9]+") {
            let raw = format!("{}/{}", first, second);
            let expected = ImageName::User { user: first, image: second};
            prop_assert_eq!(ImageName::parse(&raw), Some(expected));
        }
    }

    #[test]
    fn parses_image() {
        assert_eq!(
            "ubuntu:14.04".parse(),
            Ok(Image {
                name: ImageName::new(None, "ubuntu".to_string()),
                tag: "14.04".to_string()
            })
        )
    }

    #[test]
    fn rejects_invalid_image() {
        assert_eq!("i/am/invalid".parse::<Image>(), Err(()))
    }

    #[test]
    fn rejects_invalid_name() {
        assert_eq!(ImageName::parse("i/am/invalid"), None);
    }
}
