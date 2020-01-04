use nom::branch::alt;
use nom::bytes::complete::{tag, take_while1};
use nom::combinator::{opt, recognize};
use nom::multi::many0;
use nom::sequence::tuple;
use nom::IResult;

pub fn pattern<'a>(i: &'a str) -> IResult<&str, Vec<PatternPart<'a>>> {
    let (o, (maybe_first, mut rest)) = tuple((
        opt(outer_literal),
        many0(alt((inner_literal, version_part))),
    ))(i)?;

    if let Some(first) = maybe_first {
        rest.insert(0, first);
    };

    Ok((o, rest))
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PatternPart<'a> {
    VersionPart,
    Literal(&'a str),
}

pub fn inner_literal<'a>(i: &'a str) -> IResult<&str, PatternPart<'a>> {
    let (o, literal) = take_while1(is_inner_literal)(i)?;
    Ok((o, PatternPart::Literal(literal)))
}

pub fn outer_literal<'a>(i: &'a str) -> IResult<&str, PatternPart<'a>> {
    let (o, literal) = recognize(tuple((
        take_while1(is_outer_literal),
        take_while1(is_inner_literal),
    )))(i)?;
    Ok((o, PatternPart::Literal(literal)))
}

pub fn is_inner_literal(c: char) -> bool {
    is_outer_literal(c) || c == '.' || c == '-'
}

pub fn is_outer_literal(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn version_part<'a>(i: &'a str) -> IResult<&str, PatternPart<'a>> {
    let (o, _) = tag("<>")(i)?;
    Ok((o, PatternPart::VersionPart))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_literal() {
        assert_eq!(
            pattern("1.2.3"),
            Ok(("", vec![PatternPart::Literal("1.2.3")]))
        );
    }

    #[test]
    fn parses_version_part() {
        assert_eq!(pattern("<>"), Ok(("", vec![PatternPart::VersionPart])))
    }

    #[test]
    fn parses_semver() {
        use PatternPart::*;
        assert_eq!(
            pattern("<>.<>.<>"),
            Ok((
                "",
                vec![
                    VersionPart,
                    Literal("."),
                    VersionPart,
                    Literal("."),
                    VersionPart
                ]
            ))
        )
    }
}
