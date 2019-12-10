use std::fmt;

use regex::Regex;

#[derive(Debug, PartialEq, Eq)]
pub struct ImageName {
    user: String,
    image: String,
}

impl ImageName {
    pub fn new(image: &str) -> Option<ImageName> {
        let name_regex =
            Regex::new(r"^((?P<first>[[:word:]]+)/)?(?P<second>[[:word:]]+)$").unwrap();
        name_regex.captures(image).map(|captures| {
            let first = captures
                .name("first")
                .map(|s| s.as_str().into())
                .unwrap_or_else(|| "library".into());
            let second = captures["second"].into(); // Second group is not optional.
            ImageName {
                user: first,
                image: second,
            }
        })
    }
}

impl fmt::Display for ImageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.user, self.image)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parses_valid_short_name(image in r"[[:word:]]") {
            let expected = ImageName { user: "library".into(), image: image.clone()};
            prop_assert_eq!(ImageName::new(&image), Some(expected));
        }

        #[test]
        fn parses_valid_full_name(first in r"[[:word:]]", second in r"[[:word:]]") {
            let raw = format!("{}/{}", first, second);
            let expected = ImageName { user: first, image: second};
            prop_assert_eq!(ImageName::new(&raw), Some(expected));
        }
    }

    #[test]
    fn rejects_invalid_name() {
        assert_eq!(ImageName::new("i/am/invalid"), None);
    }
}
