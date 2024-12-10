use eyre::{bail, eyre, Result};
use jiff::Zoned;
use log::debug;
use ordered_float::OrderedFloat;
use std::collections::HashMap;
use std::fmt::{self, Display};
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

use crate::util::word;
use crate::Stream;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum UnsignedIntegerType {
    U8,
    U16,
    U32,
    U64,
    USize,
}

impl Display for UnsignedIntegerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            UnsignedIntegerType::U8 => "u8",
            UnsignedIntegerType::U16 => "u16",
            UnsignedIntegerType::U32 => "u32",
            UnsignedIntegerType::U64 => "u64",
            UnsignedIntegerType::USize => "usize",
        };
        write!(f, "{}", repr)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum SignedIntegerType {
    I8,
    I16,
    I32,
    I64,
    ISize,
}

impl Display for SignedIntegerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            SignedIntegerType::I8 => "i8",
            SignedIntegerType::I16 => "i16",
            SignedIntegerType::I32 => "i32",
            SignedIntegerType::I64 => "i64",
            SignedIntegerType::ISize => "isize",
        };
        write!(f, "{}", repr)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum IntegerType {
    Auto,
    Unsigned(UnsignedIntegerType),
    Signed(SignedIntegerType),
}

impl Display for IntegerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            IntegerType::Auto => "Integer",
            IntegerType::Unsigned(i) => &i.to_string(),
            IntegerType::Signed(i) => &i.to_string(),
        };
        write!(f, "{}", repr)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum FloatType {
    Auto,
    F32,
    F64,
}

impl Display for FloatType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            FloatType::Auto => "Float",
            FloatType::F32 => "F32",
            FloatType::F64 => "F64",
        };
        write!(f, "{}", repr)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum DataType {
    Null,
    Integer(IntegerType),
    Float(FloatType),
    String,
    Bool,
    Datetime,
    Blob,
    Enum,
    Path,
}

impl Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let repr = match self {
            DataType::Null => "NULL",
            DataType::Integer(i) => &i.to_string(),
            DataType::Float(f) => &f.to_string(),
            DataType::String => "String",
            DataType::Bool => "Bool",
            DataType::Datetime => "Datetime",
            DataType::Blob => "Blob",
            DataType::Enum => "Enum",
            DataType::Path => "Path",
        };
        write!(f, "{}", repr)
    }
}

fn integer_type<'a>(s: &mut Stream<'a>) -> PResult<IntegerType> {
    dispatch! {word;
        "INTEGER" | "INT" | "integer" | "int" | "Integer" | "Int" => empty.value(IntegerType::Auto),
        "u8" | "U8" => empty.value(IntegerType::Unsigned(UnsignedIntegerType::U8)),
        "u16" | "U16" => empty.value(IntegerType::Unsigned(UnsignedIntegerType::U16)),
        "u32" | "U32" => empty.value(IntegerType::Unsigned(UnsignedIntegerType::U32)),
        "u64" | "U64" => empty.value(IntegerType::Unsigned(UnsignedIntegerType::U64)),
        "usize" | "USIZE" => empty.value(IntegerType::Unsigned(UnsignedIntegerType::USize)),
        "i8" | "I8" => empty.value(IntegerType::Signed(SignedIntegerType::I8)),
        "i16" | "I16" => empty.value(IntegerType::Signed(SignedIntegerType::I16)),
        "i32" | "I32" => empty.value(IntegerType::Signed(SignedIntegerType::I32)),
        "i64" | "I64" => empty.value(IntegerType::Signed(SignedIntegerType::I64)),
        "isize" | "ISIZE" => empty.value(IntegerType::Signed(SignedIntegerType::ISize)),
        _ => fail::<_, IntegerType, _>,
    }
    .parse_next(s)
}

fn float_type<'a>(s: &mut Stream<'a>) -> PResult<FloatType> {
    dispatch! {word;
        "REAL" | "real" | "Real" => empty.value(FloatType::Auto),
        "FLOAT" | "float" | "Float" => empty.value(FloatType::F32),
        "DOUBLE" | "double" | "Double" => empty.value(FloatType::F64),
        "F32" | "f32" => empty.value(FloatType::F32),
        "F64" | "f64" => empty.value(FloatType::F64),
        _ => fail::<_, FloatType, _>,
    }
    .parse_next(s)
}

fn string_type<'a>(s: &mut Stream<'a>) -> PResult<DataType> {
    alt((
        "STRING", "string", "String", "TEXT", "text", "Text", "VARCHAR", "varchar", "Varchar",
        "STR", "str", "Str",
    ))
    .map(|_| DataType::String)
    .parse_next(s)
}

fn bool_type<'a>(s: &mut Stream<'a>) -> PResult<DataType> {
    alt(("BOOLEAN", "boolean", "Boolean", "BOOL", "bool", "bool"))
        .map(|_| DataType::Bool)
        .parse_next(s)
}

fn datetime_type<'a>(s: &mut Stream<'a>) -> PResult<DataType> {
    alt(("DATETIME", "datetime", "Datetime", "DATE", "date", "Date"))
        .map(|_| DataType::Datetime)
        .parse_next(s)
}

fn blob_type<'a>(s: &mut Stream<'a>) -> PResult<DataType> {
    alt(("BLOB", "blob", "Blob"))
        .map(|_| DataType::Blob)
        .parse_next(s)
}

fn null_type<'a>(s: &mut Stream<'a>) -> PResult<DataType> {
    alt(("NULL", "null")).map(|_| DataType::Null).parse_next(s)
}

pub fn data_type<'a>(s: &mut Stream<'a>) -> PResult<DataType> {
    // TODO: this should be a dispatch call
    alt((
        integer_type.map(|i| DataType::Integer(i)),
        float_type.map(|f| DataType::Float(f)),
        string_type,
        bool_type,
        datetime_type,
        null_type,
        blob_type,
    ))
    .parse_next(s)
}
