use eyre::{bail, eyre, Result};
use jiff::Zoned;
use log::debug;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use winnow::ascii::{alpha1, dec_int, dec_uint, digit1, float, multispace0};
use winnow::binary::length_take;
use winnow::combinator::{cut_err, delimited, dispatch, repeat, DefaultValue};
use winnow::combinator::{empty, fail, peek};
use winnow::combinator::{opt, separated};
use winnow::error::{
    AddContext, ContextError, ErrMode, ErrorKind, FromExternalError, ParseError, StrContext,
    StrContextValue,
};
use winnow::prelude::*;
use winnow::stream::AsChar;
use winnow::stream::Stateful;
use winnow::token::{take_till, take_while};
use winnow::{ascii::multispace1, combinator::alt};

use crate::Stream;

/// Identifier
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct Ident {
    ident: String,
}

impl Ident {
    pub fn from_str(ident: &str) -> Self {
        Self {
            ident: ident.to_string(),
        }
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ident)
    }
}

pub fn ident<'a>(s: &mut Stream<'a>) -> PResult<Ident> {
    // First character must be either an alphabatic or underscore (can't be a number)
    let _ = peek(alt((alpha1, "_"))).parse_next(s)?;
    take_while(1.., |c: char| c.is_alphanumeric() || c == '_')
        .map(|s| Ident::from_str(s))
        .parse_next(s)
}
