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
// make it unnecessarily complex. We will, however, allow only the specified
// character set.
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
    fn rejects_invalid_name() {
        assert_eq!(ImageName::parse("i/am/invalid"), None);
    }
}
