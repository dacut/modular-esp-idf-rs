#![allow(clippy::result_large_err)]

use {
    pest::{
        error::{Error, ErrorVariant},
        iterators::Pair,
    },
    pest_derive::Parser,
    std::borrow::Cow,
};

macro_rules! try_from_pairs {
    ($name:ident, $rule:ident) => {
        impl<'a> ::std::convert::TryFrom<::pest::iterators::Pairs<'a, $crate::Rule>> for $name<'a> {
            type Error = ::pest::error::Error<$crate::Rule>;

            fn try_from(mut pairs: ::pest::iterators::Pairs<'a, $crate::Rule>) -> Result<Self, Self::Error> {
                let pair = pairs.next().unwrap();
                assert!(pairs.next().is_none());

                Self::try_from(pair)
            }
        }
    };
}

macro_rules! check_rule {
    ($pair:ident, $rule:ident) => {
        if !matches!($pair.as_rule(), $crate::Rule::$rule) {
            return ::std::result::Result::Err(
                ::pest::error::Error::new_from_span(
                    ::pest::error::ErrorVariant::CustomError {
                        message: format!("not a {:?}: {}", $crate::Rule::$rule, $pair),
                    },
                    $pair.as_span(),
                )
            );
        }
    }
}

#[derive(Parser)]
#[grammar = "kconfig.pest"]
pub struct KConfigFile<'a> {
    pub blocks: Vec<TopLevel<'a>>,
}

impl<'a> TryFrom<Pair<'a, Rule>> for KConfigFile<'a> {
    type Error = Error<Rule>;

    fn try_from(pair: Pair<'a, Rule>) -> Result<Self, Error<Rule>> {
        check_rule!(pair, file);

        let mut blocks = Vec::new();
        for pair in pair.into_inner() {
            blocks.push(TopLevel::try_from(pair)?);
        }

        Ok(Self {
            blocks,
        })
    }
}

try_from_pairs!(KConfigFile, file);

#[derive(Debug, Eq, PartialEq)]
pub enum TopLevel<'a> {
    SourceDirective(SourceDirective<'a>),
}

impl<'a> TryFrom<Pair<'a, Rule>> for TopLevel<'a> {
    type Error = Error<Rule>;

    fn try_from(pair: Pair<'a, Rule>) -> Result<Self, Error<Rule>> {
        check_rule!(pair, top_level);

        let mut pairs = pair.into_inner();
        let pair = pairs.next().unwrap();
        assert!(pairs.next().is_none());

        match pair.as_rule() {
            Rule::source_directive => Ok(Self::SourceDirective(SourceDirective::try_from(pair).unwrap())),
            _ => unreachable!("not a top-level: {pair:?}"),
        }
    }
}

/// The type of a source directive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceType {
    /// `source` directive: read the specified file(s).
    Source,

    /// `rsource` directive: read the specified file(s) relative to the current file.
    RSource,

    /// `osource` directive: read the specified file(s) if they exist.
    OSource,

    /// `orsource` directive: read the specified file(s) relative to the current file if they exist.
    ORSource,
}

impl SourceType {
    #[inline(always)]
    pub fn is_optional(&self) -> bool {
        matches!(self, Self::OSource | Self::ORSource)
    }

    #[inline(always)]
    pub fn is_relative(&self) -> bool {
        matches!(self, Self::RSource | Self::ORSource)
    }
}

impl TryFrom<Pair<'_, Rule>> for SourceType {
    type Error = Error<Rule>;

    fn try_from(pair: Pair<'_, Rule>) -> Result<Self, Error<Rule>> {
        let rule = pair.as_rule();
        match rule {
            Rule::K_SOURCE => Ok(Self::Source),
            Rule::K_RSOURCE => Ok(Self::RSource),
            Rule::K_OSOURCE => Ok(Self::OSource),
            Rule::K_ORSOURCE => Ok(Self::ORSource),
            Rule::source_token => Self::try_from(pair.into_inner().next().unwrap()),
            _ => Err(Error::new_from_span(
                ErrorVariant::CustomError {
                    message: format!("not a source token: {pair:?}"),
                },
                pair.as_span(),
            )),
        }
    }
}

/// A source directive. One of:
/// - `source "filename"`
/// - `rsource "filename"`
/// - `osource "filename"`
/// - `orsource "filename"`
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDirective<'a> {
    pub source_type: SourceType,
    pub filename_glob: Cow<'a, str>,
}

impl<'a> TryFrom<Pair<'a, Rule>> for SourceDirective<'a> {
    type Error = Error<Rule>;

    fn try_from(pair: Pair<'a, Rule>) -> Result<Self, Error<Rule>> {
        check_rule!(pair, source_directive);

        let mut pairs = pair.into_inner();
        let source_type = SourceType::try_from(pairs.next().unwrap())?;
        let filename_glob = parse_string_literal(&pairs.next().unwrap())?;
        Ok(Self {
            source_type,
            filename_glob,
        })
    }
}

fn parse_string_literal<'a>(pair: &Pair<'a, Rule>) -> Result<Cow<'a, str>, Error<Rule>> {
    assert_eq!(pair.as_rule(), Rule::string);
    let literal = pair.as_str();
    assert!(literal.len() >= 2);
    assert!(literal.starts_with('"'));
    assert!(literal.ends_with('"'));

    let literal = &literal[1..literal.len() - 1];
    if !literal.contains('\\') {
        return Ok(Cow::Borrowed(literal));
    }

    let mut result = String::with_capacity(literal.len() - 2);
    let mut chars = literal[1..literal.len() - 1].chars();
    loop {
        let c = chars.next().unwrap();
        if c != '\\' {
            result.push(c);
            continue;
        }

        let c = chars.next().unwrap();
        match c {
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            '\\' => result.push('\\'),
            '0' => result.push('\0'),
            '\'' => result.push('\''),
            '"' => result.push('"'),
            'x' => {
                let mut hex = String::with_capacity(2);
                hex.push(chars.next().unwrap());
                hex.push(chars.next().unwrap());
                let c = u8::from_str_radix(&hex, 16).map_err(|_| {
                    Error::new_from_span(
                        ErrorVariant::CustomError {
                            message: format!("invalid hex escape: \\x{hex}"),
                        },
                        pair.as_span(),
                    )
                })?;
                result.push(c as char);
            }
            'u' => {
                assert_eq!(chars.next().unwrap(), '{');
                let mut hex = String::with_capacity(6);
                loop {
                    let c = chars.next().unwrap();
                    if c == '}' {
                        break;
                    }
                    hex.push(c);
                }
                let c = u32::from_str_radix(&hex, 16).map_err(|_| {
                    Error::new_from_span(
                        ErrorVariant::CustomError {
                            message: format!("invalid unicode escape: \\u{{{hex}}}"),
                        },
                        pair.as_span(),
                    )
                })?;
                let c = char::from_u32(c).ok_or_else(|| {
                    Error::new_from_span(
                        ErrorVariant::CustomError {
                            message: format!("invalid unicode codepoint: \\u{{{hex}}}"),
                        },
                        pair.as_span(),
                    )
                })?;
                result.push(c);
            }
            _ => {
                return Err(Error::new_from_span(
                    ErrorVariant::CustomError {
                        message: format!("invalid escape: \\{}", c),
                    },
                    pair.as_span(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod config {
    use {super::*, pest::Parser};

    #[test]
    fn test_source() {
        let result = KConfigFile::parse(Rule::file, "source \"foo\"\n\nsource\t\"bar\"\t\n");
        let result = result.unwrap();

        let file = KConfigFile::try_from(result).unwrap();
        assert_eq!(file.blocks.len(), 2);

        for block in file.blocks.iter() {
            assert!(matches!(block, TopLevel::SourceDirective(_)));
        }
    }
}
