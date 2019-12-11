use regex::Regex;

#[derive(Debug, PartialEq, Eq)]
pub enum ImageName {
    Official { image: String },
    User { user: String, image: String },
}

impl ImageName {
    pub fn new(image: &str) -> Option<ImageName> {
        let name_regex =
            Regex::new(r"^((?P<first>[[:word:]]+)/)?(?P<second>[[:word:]]+)$").unwrap();
        name_regex.captures(image).map(|captures| {
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

#[cfg(test)]
mod test {
    use super::*;

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parses_valid_official_name(image in r"[[:word:]]") {
            let expected = ImageName::Official { image: image.clone()};
            prop_assert_eq!(ImageName::new(&image), Some(expected));
        }

        #[test]
        fn parses_valid_user_name(first in r"[[:word:]]", second in r"[[:word:]]") {
            let raw = format!("{}/{}", first, second);
            let expected = ImageName::User { user: first, image: second};
            prop_assert_eq!(ImageName::new(&raw), Some(expected));
        }
    }

    #[test]
    fn rejects_invalid_name() {
        assert_eq!(ImageName::new("i/am/invalid"), None);
    }
}
