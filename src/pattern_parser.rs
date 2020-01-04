use itertools::Itertools;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_while1};
use nom::combinator::{all_consuming, map, opt, recognize};
use nom::error::{ParseError, VerboseError};
use nom::multi::many0;
use nom::sequence::tuple;
use nom::IResult;
use regex::Regex;
use thiserror::Error;

pub fn pattern<'a, E>(i: &'a str) -> IResult<&'a str, Pattern, E>
where
    E: ParseError<&'a str>,
{
    map(
        all_consuming(tuple((
            opt(outer_literal),
            many0(alt((inner_literal, version_part))),
        ))),
        |(maybe_first, mut rest)| {
            if let Some(first) = maybe_first {
                rest.insert(0, first);
            };

            Pattern(rest)
        },
    )(i)
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Pattern(Vec<PatternPart>);

impl Pattern {
    pub fn parse(i: &str) -> Result<Pattern, Error> {
        pattern(i)
            .map(|(_, pattern)| pattern)
            .map_err(|error| Error::new(i, error))
    }

    pub fn regex(&self) -> Regex {
        use PatternPart::*;
        let inner_regex = self
            .0
            .iter()
            .map(|part| match part {
                Literal(literal) => Self::escape_literal(&literal),
                VersionPart => r"(\d+)".to_string(),
            })
            .join("");
        let raw_regex = format!("^{}$", inner_regex);

        Regex::new(&raw_regex).unwrap()
    }

    fn escape_literal(literal: &str) -> String {
        literal.replace(".", r"\.")
    }
}

#[derive(Debug, PartialEq, Error)]
#[error("{description}")]
pub struct Error {
    description: String,
}

impl Error {
    pub fn new(input: &str, error: nom::Err<VerboseError<&str>>) -> Self {
        use nom::Err::*;
        let description = match error {
            Incomplete(_) => "Incomplete input".to_string(),
            Failure(error) | Error(error) => nom::error::convert_error(input, error),
        };
        Self { description }
    }
}

impl std::fmt::Display for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.0
                .iter()
                .map(|part| {
                    use PatternPart::*;
                    match part {
                        VersionPart => "<>".to_string(),
                        Literal(literal) => literal.clone(),
                    }
                })
                .join("")
        )
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PatternPart {
    VersionPart,
    Literal(String),
}

pub fn inner_literal<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
where
    E: ParseError<&'a str>,
{
    let (o, literal) = take_while1(is_inner_literal)(i)?;
    Ok((o, PatternPart::Literal(literal.to_string())))
}

pub fn outer_literal<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
where
    E: ParseError<&'a str>,
{
    let (o, literal) = recognize(tuple((
        take_while1(is_outer_literal),
        take_while1(is_inner_literal),
    )))(i)?;
    Ok((o, PatternPart::Literal(literal.to_string())))
}

pub fn is_inner_literal(c: char) -> bool {
    is_outer_literal(c) || c == '.' || c == '-'
}

pub fn is_outer_literal(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn version_part<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
where
    E: ParseError<&'a str>,
{
    let (o, _) = tag("<>")(i)?;
    Ok((o, PatternPart::VersionPart))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_literal() {
        assert_eq!(
            Pattern::parse("1.2.3"),
            Ok(Pattern(vec![PatternPart::Literal("1.2.3".to_string())]))
        );
    }

    #[test]
    fn parses_version_part() {
        assert_eq!(
            Pattern::parse("<>"),
            Ok(Pattern(vec![PatternPart::VersionPart]))
        )
    }

    #[test]
    fn parses_semver() {
        use PatternPart::*;
        assert_eq!(
            Pattern::parse("<>.<>.<>"),
            Ok(Pattern(vec![
                VersionPart,
                Literal(".".to_string()),
                VersionPart,
                Literal(".".to_string()),
                VersionPart
            ]))
        )
    }

    #[test]
    fn rejects_invalid_pattern() {
        assert_eq!(
            pattern(r"(\d+)\.(\d+)"),
            Err(nom::Err::Error((
                r"(\d+)\.(\d+)",
                nom::error::ErrorKind::Eof
            )))
        );
    }
}
