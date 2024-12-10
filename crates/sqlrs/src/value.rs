use eyre::{bail, eyre, Result};
use jiff::Zoned;
use log::debug;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Display;
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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum FloatValue {
    Auto(OrderedFloat<f64>),
    F32(OrderedFloat<f32>),
    F64(OrderedFloat<f64>),
}

impl FloatValue {
    fn ttype(&self) -> FloatType {
        match self {
            FloatValue::Auto(_) => FloatType::Auto,
            FloatValue::F32(_) => FloatType::F32,
            FloatValue::F64(_) => FloatType::F64,
        }
    }
}

impl Display for FloatValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            FloatValue::Auto(f) => f.to_string(),
            FloatValue::F32(f) => f.to_string(),
            FloatValue::F64(f) => f.to_string(),
        };
        write!(f, "{}", repr)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum UnsignedIntegerValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    USize(usize),
}

impl UnsignedIntegerValue {
    fn ttype(&self) -> UnsignedIntegerType {
        match self {
            UnsignedIntegerValue::U8(_) => UnsignedIntegerType::U8,
            UnsignedIntegerValue::U16(_) => UnsignedIntegerType::U16,
            UnsignedIntegerValue::U32(_) => UnsignedIntegerType::U32,
            UnsignedIntegerValue::U64(_) => UnsignedIntegerType::U64,
            UnsignedIntegerValue::USize(_) => UnsignedIntegerType::USize,
        }
    }
}

impl Display for UnsignedIntegerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            UnsignedIntegerValue::U8(i) => i.to_string(),
            UnsignedIntegerValue::U16(i) => i.to_string(),
            UnsignedIntegerValue::U32(i) => i.to_string(),
            UnsignedIntegerValue::U64(i) => i.to_string(),
            UnsignedIntegerValue::USize(i) => i.to_string(),
        };
        write!(f, "{}", repr)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum SignedIntegerValue {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    ISize(isize),
}

impl SignedIntegerValue {
    fn ttype(&self) -> SignedIntegerType {
        match self {
            SignedIntegerValue::I8(_) => SignedIntegerType::I8,
            SignedIntegerValue::I16(_) => SignedIntegerType::I16,
            SignedIntegerValue::I32(_) => SignedIntegerType::I32,
            SignedIntegerValue::I64(_) => SignedIntegerType::I64,
            SignedIntegerValue::ISize(_) => SignedIntegerType::ISize,
        }
    }
}

impl Display for SignedIntegerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            SignedIntegerValue::I8(i) => i.to_string(),
            SignedIntegerValue::I16(i) => i.to_string(),
            SignedIntegerValue::I32(i) => i.to_string(),
            SignedIntegerValue::I64(i) => i.to_string(),
            SignedIntegerValue::ISize(i) => i.to_string(),
        };
        write!(f, "{}", repr)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum IntegerValue {
    Auto(i32),
    Unsigned(UnsignedIntegerValue),
    Signed(SignedIntegerValue),
}

impl IntegerValue {
    fn ttype(&self) -> IntegerType {
        match self {
            Self::Auto(_) => IntegerType::Auto,
            Self::Unsigned(i) => IntegerType::Unsigned(i.ttype()),
            Self::Signed(i) => IntegerType::Signed(i.ttype()),
        }
    }
}

impl Display for IntegerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            IntegerValue::Auto(i) => i.to_string(),
            IntegerValue::Unsigned(i) => i.to_string(),
            IntegerValue::Signed(i) => i.to_string(),
        };
        write!(f, "{}", repr)
    }
}

use ordered_float::OrderedFloat;

use crate::r#type::{DataType, FloatType, IntegerType, SignedIntegerType, UnsignedIntegerType};
use crate::util::quoted;
use crate::Stream;
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Null,
    Integer(IntegerValue),
    Float(FloatValue),
    String(String),
    Bool(bool),
    Datetime(Zoned),
    Blob(String),
    Enum(String),
    Path(PathBuf),
}

impl Value {
    fn ttype(&self) -> DataType {
        match self {
            Value::Null => DataType::Null,
            Value::Integer(i) => DataType::Integer(i.ttype()),
            Value::Float(f) => DataType::Float(f.ttype()),
            Value::String(_) => DataType::String,
            Value::Bool(_) => DataType::Bool,
            Value::Datetime(_) => DataType::Datetime,
            Value::Blob(_) => DataType::Blob,
            Value::Enum(_) => DataType::Enum,
            Value::Path(_) => DataType::Path,
        }
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            Value::Null => "NULL",
            Value::Integer(i) => &i.to_string(),
            Value::Float(f) => &f.to_string(),
            Value::String(s) => &s,
            Value::Bool(b) => match b {
                true => "true",
                false => "false",
            },
            Value::Datetime(d) => &d.to_string(),
            Value::Blob(b) => &b,
            Value::Enum(e) => e,
            Value::Path(p) => &p.display().to_string(),
        };
        write!(f, "{}", repr)
    }
}

// TODO: replace all my alt((literal_str, literal_str, ...)) calls with dispatch

fn null_value<'a>(s: &mut Stream<'a>) -> PResult<()> {
    alt(("NULL", "null", "Null")).parse_next(s)?;
    Ok(())
}

fn float_value<'a>(float_type: FloatType) -> impl FnMut(&mut Stream) -> PResult<FloatValue> {
    move |s: &mut Stream| {
        let value = match float_type {
            FloatType::Auto => {
                let value: f64 = float.parse_next(s)?;
                FloatValue::Auto(OrderedFloat(value))
            }
            FloatType::F64 => {
                let value: f64 = float.parse_next(s)?;
                FloatValue::F64(OrderedFloat(value))
            }
            FloatType::F32 => {
                let value: f32 = float.parse_next(s)?;
                FloatValue::F32(OrderedFloat(value))
            }
        };
        Ok(value)
    }
}

fn unsigned_integer_value<'a>(
    unsigned_type: UnsignedIntegerType,
) -> impl FnMut(&mut Stream) -> PResult<UnsignedIntegerValue> {
    move |s: &mut Stream| {
        let value = match unsigned_type {
            UnsignedIntegerType::U8 => {
                let value: u8 = dec_uint.parse_next(s)?;
                UnsignedIntegerValue::U8(value)
            }
            UnsignedIntegerType::U16 => {
                let value: u16 = dec_uint.parse_next(s)?;
                UnsignedIntegerValue::U16(value)
            }
            UnsignedIntegerType::U32 => {
                let value: u32 = dec_uint.parse_next(s)?;
                UnsignedIntegerValue::U32(value)
            }
            UnsignedIntegerType::U64 => {
                let value: u64 = dec_uint.parse_next(s)?;
                UnsignedIntegerValue::U64(value)
            }
            UnsignedIntegerType::USize => {
                let value: usize = dec_uint.parse_next(s)?;
                UnsignedIntegerValue::USize(value)
            }
        };
        Ok(value)
    }
}

fn signed_integer_value<'a>(
    signed_type: SignedIntegerType,
) -> impl FnMut(&mut Stream) -> PResult<SignedIntegerValue> {
    move |s: &mut Stream| {
        let value = match signed_type {
            SignedIntegerType::I8 => {
                let value: i8 = dec_int.parse_next(s)?;
                SignedIntegerValue::I8(value)
            }
            SignedIntegerType::I16 => {
                let value: i16 = dec_int.parse_next(s)?;
                SignedIntegerValue::I16(value)
            }
            SignedIntegerType::I32 => {
                let value: i32 = dec_int.parse_next(s)?;
                SignedIntegerValue::I32(value)
            }
            SignedIntegerType::I64 => {
                let value: i64 = dec_int.parse_next(s)?;
                SignedIntegerValue::I64(value)
            }
            SignedIntegerType::ISize => {
                let value: isize = dec_int.parse_next(s)?;
                SignedIntegerValue::ISize(value)
            }
        };
        Ok(value)
    }
}

fn integer_value<'a>(
    integer_type: IntegerType,
) -> impl FnMut(&mut Stream) -> PResult<IntegerValue> {
    move |s: &mut Stream| {
        let value = match integer_type {
            IntegerType::Auto => {
                let value: i32 = dec_int.parse_next(s)?;
                IntegerValue::Auto(value)
            }
            IntegerType::Unsigned(u_type) => unsigned_integer_value(u_type)
                .map(|u| IntegerValue::Unsigned(u))
                .parse_next(s)?,
            IntegerType::Signed(i_type) => signed_integer_value(i_type)
                .map(|i| IntegerValue::Signed(i))
                .parse_next(s)?,
        };
        Ok(value)
    }
}

pub fn value<'a>(ttype: DataType) -> impl FnMut(&mut Stream) -> PResult<Value> {
    move |s: &mut Stream| {
        let value = match ttype {
            DataType::Null => null_value.map(|_| Value::Null).parse_next(s)?,
            DataType::Integer(int_type) => integer_value(int_type)
                .map(|i| Value::Integer(i))
                .parse_next(s)?,
            DataType::Float(float_type) => float_value(float_type)
                .map(|f| Value::Float(f))
                .parse_next(s)?,
            DataType::String => string_value
                .map(|s| Value::String(s.to_string()))
                .parse_next(s)?,
            DataType::Bool => bool_value.map(|b| Value::Bool(b)).parse_next(s)?,
            DataType::Datetime => datetime_value.map(|d| Value::Datetime(d)).parse_next(s)?,
            DataType::Blob => blob_value
                .map(|b| Value::Blob(b.to_string()))
                .parse_next(s)?,
            DataType::Enum => enum_value
                .map(|e| Value::Enum(e.to_string()))
                .parse_next(s)?,
            DataType::Path => path_value.map(|p| Value::Path(p)).parse_next(s)?,
        };
        Ok(value)
    }
}

fn enum_value<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    todo!();
}

fn path_value<'a>(s: &mut Stream<'a>) -> PResult<PathBuf> {
    todo!();
}

fn string_value<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    quoted.parse_next(s)
}

fn ttrue<'a>(s: &mut Stream<'a>) -> PResult<bool> {
    alt(("TRUE", "true", "True")).map(|_| true).parse_next(s)
}

fn ffalse<'a>(s: &mut Stream<'a>) -> PResult<bool> {
    alt(("FALSE", "false", "False"))
        .map(|_| false)
        .parse_next(s)
}

fn bool_value<'a>(s: &mut Stream<'a>) -> PResult<bool> {
    alt((ttrue, ffalse)).parse_next(s)
}

fn datetime_value<'a>(s: &mut Stream<'a>) -> PResult<Zoned> {
    todo!();
}

fn blob_value<'a>(s: &mut Stream<'a>) -> PResult<&'a str> {
    todo!();
}
