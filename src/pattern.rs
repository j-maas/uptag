use itertools::Itertools;
use thiserror::Error;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Pattern {
    parts: Vec<PatternPart>,
    breaking_degree: usize,
}

impl Pattern {
    pub fn parse(i: &str) -> Result<Pattern, Error> {
        parser::pattern(i)
            .map(|(_, pattern)| pattern)
            .map_err(|error| Error::new(i, error))
    }

    pub fn parts(&self) -> &Vec<PatternPart> {
        &self.parts
    }

    pub fn breaking_degree(&self) -> usize {
        self.breaking_degree
    }
}

#[derive(Debug, PartialEq, Error)]
#[error("{description}")]
pub struct Error {
    description: String,
}

impl Error {
    pub fn new(input: &str, error: parser::Error) -> Self {
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
        let mut version_part_counter = 0;
        write!(
            f,
            "{}",
            self.parts
                .iter()
                .map(|part| {
                    use PatternPart::*;
                    match part {
                        VersionPart => {
                            version_part_counter += 1;
                            if version_part_counter <= self.breaking_degree() {
                                "<!>".to_string()
                            } else {
                                "<>".to_string()
                            }
                        }
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

mod parser {
    use super::*;

    use nom::branch::alt;
    use nom::bytes::complete::{tag, take_while1};
    use nom::combinator::{all_consuming, opt, recognize};
    use nom::error::ParseError;
    use nom::multi::many0;
    use nom::sequence::tuple;
    use nom::IResult;

    pub type Error<'a> = nom::Err<nom::error::VerboseError<&'a str>>;

    pub fn pattern<'a, E>(i: &'a str) -> IResult<&'a str, Pattern, E>
    where
        E: ParseError<&'a str>,
    {
        let (o, (maybe_first, mut breaking, mut compatible)) = all_consuming(tuple((
            opt(outer_literal),
            breaking_parts,
            compatible_parts,
        )))(i)?;

        let breaking_degree = breaking
            .iter()
            .filter(|part| matches!(part, PatternPart::VersionPart))
            .count();
        let mut parts = match maybe_first {
            Some(first) => vec![first],
            None => vec![],
        };
        parts.append(&mut breaking);
        parts.append(&mut compatible);
        Ok((
            o,
            Pattern {
                parts,
                breaking_degree,
            },
        ))
    }

    pub fn breaking_parts<'a, E>(i: &'a str) -> IResult<&'a str, Vec<PatternPart>, E>
    where
        E: ParseError<&'a str>,
    {
        many0(alt((inner_literal, breaking_version_part)))(i)
    }

    pub fn compatible_parts<'a, E>(i: &'a str) -> IResult<&'a str, Vec<PatternPart>, E>
    where
        E: ParseError<&'a str>,
    {
        many0(alt((inner_literal, compatible_version_part)))(i)
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

    pub fn breaking_version_part<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
    where
        E: ParseError<&'a str>,
    {
        let (o, _) = tag("<!>")(i)?;
        Ok((o, PatternPart::VersionPart))
    }

    pub fn compatible_version_part<'a, E>(i: &'a str) -> IResult<&'a str, PatternPart, E>
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
                Ok(Pattern {
                    parts: vec![PatternPart::Literal("1.2.3".to_string())],
                    breaking_degree: 0
                })
            );
        }

        #[test]
        fn parses_version_part() {
            assert_eq!(
                Pattern::parse("<>"),
                Ok(Pattern {
                    parts: vec![PatternPart::VersionPart],
                    breaking_degree: 0
                })
            )
        }

        #[test]
        fn parses_semver() {
            use PatternPart::*;
            assert_eq!(
                Pattern::parse("<!>.<>.<>"),
                Ok(Pattern {
                    parts: vec![
                        VersionPart,
                        Literal(".".to_string()),
                        VersionPart,
                        Literal(".".to_string()),
                        VersionPart
                    ],
                    breaking_degree: 1
                })
            )
        }

        #[test]
        fn rejects_invalid_characters() {
            assert_eq!(
                pattern(r"(\d+)\.(\d+)"),
                Err(nom::Err::Error((
                    r"(\d+)\.(\d+)",
                    nom::error::ErrorKind::Eof
                )))
            );
        }

        #[test]
        fn rejects_invalid_break_indicator() {
            assert_eq!(pattern(r"<>.<!>.<>"), Err(nom::Err::Error(())))
        }
    }
}
