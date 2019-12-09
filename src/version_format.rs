use regex::Regex;

/// A version format detecting and comparing versions.
    ///
    /// The syntax is based on [Regex], but adds two special escaped characters:
    /// - `\m` which stands for a semantic version (se**m**antic).
    /// - `\q` which stands for a sequence number (se**q**uence).
    ///
    /// # Examples
    ///
    /// Detect only proper SemVer, without any prefix or suffix:
    ///
    /// ```rust
    /// # extern crate updock; use updock::VersionFormat;
    /// # fn main() {
    /// let format = VersionFormat::new(r"^\m$").unwrap();
    /// assert!(format.matches("1.2.3"));
    /// assert!(!format.matches("1.2.3-debian"));
    /// # }
    /// ```
    ///
    /// Detect a sequential version after a prefix:
///
    /// ```rust
    /// # extern crate updock; use updock::VersionFormat;
    /// # fn main() {
    /// let format = VersionFormat::new(r"^debian-r\q$").unwrap();
    /// assert!(format.matches("debian-r24"));
    /// assert!(!format.matches("debian-r24-alpha"));
    /// # }
    /// ```
    ///
    /// [Regex]: https://docs.rs/regex/1.3.1/regex/index.html#syntax
#[derive(Debug)]
pub struct VersionFormat {
    regex: Regex,
}

impl VersionFormat {
    pub fn new(format: &str) -> Result<VersionFormat, regex::Error> {
        let semver_code = r"\m";
        let normalized_semver = r"(?P<semver>\d+\.\d+\.\d+)";

        let sequence_code = r"\q";
        let normalized_sequence = r"(?P<seq>\d+)";

        let normalized_format = format
            .replace(semver_code, normalized_semver)
            .replace(sequence_code, normalized_sequence);

        let regex = Regex::new(&normalized_format)?;

        Ok(VersionFormat { regex })
    }

    pub fn matches(&self, version: &str) -> bool {
        self.regex.is_match(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_matches {
        ($format:ident, $string:ident) => {
            assert!(
                $format.matches($string),
                format!("{:?} did not match '{}'.", $format, $string)
            );
        };
    }

    macro_rules! assert_no_match {
        ($format:ident, $string:ident) => {
            assert!(
                !$format.matches($string),
                format!("{:?} should not match '{}'.", $format, $string)
            );
        };
    }

    #[test]
    fn detects_simple_semver() {
        let format = VersionFormat::new(r"^\m$").unwrap();

        let matches = ["0.1.0", "1.2.3"];
        for m in &matches {
            assert_matches!(format, m);
        }

        let no_matches = ["1", "1.1", "1.1.1-alpha", "debian-1.0.1", "1.0.1-debian"];
        for m in &no_matches {
            assert_no_match!(format, m);
        }
    }

    #[test]
    fn detects_surrounded_semver() {
        let format = VersionFormat::new(r"^v\m-debian$").unwrap();

        let matches = ["v0.1.0-debian", "v1.2.3-debian"];
        for m in &matches {
            assert_matches!(format, m);
        }

        let no_matches = ["1.2.3-debian", "v1.2.3", "1.2.3"];
        for m in &no_matches {
            assert_no_match!(format, m);
        }
    }

    #[test]
    fn detects_simple_sequence() {
        let format = VersionFormat::new(r"^\q$").unwrap();

        let matches = ["1", "100"];
        for m in &matches {
            assert_matches!(format, m);
        }

        let no_matches = ["1.", "1.0", "debian-3", "123-debian"];
        for m in &no_matches {
            assert_no_match!(format, m);
        }
    }

    #[test]
    fn detects_surrounded_sequence() {
        let format = VersionFormat::new(r"^release:\q-debian$").unwrap();

        let matches = ["release:0-debian", "release:1393-debian"];
        for m in &matches {
            assert_matches!(format, m);
        }

        let no_matches = ["10-debian", "release:4", "123"];
        for m in &no_matches {
            assert_no_match!(format, m);
        }
    }
}
