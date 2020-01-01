use std::fmt;

use lazy_static::lazy_static;
use regex::Regex;
use serde::{Serialize, Serializer};

#[derive(Debug, PartialEq, Eq, Clone)]
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

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum ImageName {
    Official { image: String },
    User { user: String, image: String },
}

lazy_static! {
    static ref NAME: Regex =
        Regex::new(r"^((?P<first>[[:word:]]+)/)?(?P<second>[[:word:]]+)$").unwrap();
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

#[derive(Debug)]
pub struct ParseError {
    invalid: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a valid image name of the form `<image>` or `<user>/<image>`",
            self.invalid
        )
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod test {
    use super::*;

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parses_valid_official_name(image in r"[[:word:]]") {
            let expected = ImageName::Official { image: image.clone()};
            prop_assert_eq!(ImageName::parse(&image), Some(expected));
        }

        #[test]
        fn parses_valid_user_name(first in r"[[:word:]]", second in r"[[:word:]]") {
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
