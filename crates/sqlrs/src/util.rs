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

pub fn keyword<'a>(keyword: &'a mut &'a str) -> impl FnMut(&mut Stream) -> PResult<()> + 'a {
    move |s: &mut Stream| {
        let _ = keyword.parse_next(s)?;
        let _ = required_space.parse_next(s)?;
        Ok(())
    }
}

pub fn single_quoted_helper<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    // TODO: handle escape chars
    take_till(0.., |c| c == '\'').parse_next(s)
}

pub fn double_quoted_helper<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    // TODO: handle escape chars
    take_till(0.., |c| c == '"').parse_next(s)
}

pub fn single_quoted<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    delimited('\'', single_quoted_helper, '\'').parse_next(s)
}

pub fn double_quoted<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    delimited('"', double_quoted_helper, '"').parse_next(s)
}

pub fn quoted<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    // TODO: can we condense into one function for single quoted and double quoted thing?
    alt((single_quoted, double_quoted)).parse_next(s)
}

pub fn word<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    take_while(1.., |c: char| c.is_alphanumeric()).parse_next(s)
}

pub fn required_space<'a>(s: &mut Stream<'a>) -> PResult<()> {
    multispace1.parse_next(s)?;
    Ok(())
}

pub fn opt_space<'a>(s: &mut Stream<'a>) -> PResult<()> {
    multispace0.parse_next(s)?;
    Ok(())
}

pub fn equal_sign<'a>(s: &mut Stream<'a>) -> PResult<()> {
    let _ = opt_space.parse_next(s)?;
    let _ = "=".parse_next(s)?;
    let _ = opt_space.parse_next(s)?;
    Ok(())
}

pub fn comma<'a>(s: &mut Stream<'a>) -> PResult<()> {
    let _ = opt_space.parse_next(s)?;
    let _ = ",".parse_next(s)?;
    let _ = opt_space.parse_next(s)?;
    Ok(())
}

pub fn semi_colon<'a>(s: &mut Stream<'a>) -> PResult<()> {
    let _ = opt_space.parse_next(s)?;
    let _ = ";".parse_next(s)?;
    let _ = opt_space.parse_next(s)?;
    Ok(())
}

pub fn open_paren<'a>(s: &mut Stream<'a>) -> PResult<()> {
    let _ = opt_space.parse_next(s)?;
    let _ = "(".parse_next(s)?;
    let _ = opt_space.parse_next(s)?;
    Ok(())
}

pub fn close_paren<'a>(s: &mut Stream<'a>) -> PResult<()> {
    let _ = opt_space.parse_next(s)?;
    let _ = ")".parse_next(s)?;
    let _ = opt_space.parse_next(s)?;
    Ok(())
}
