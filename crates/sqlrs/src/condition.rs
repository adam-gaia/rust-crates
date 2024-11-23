use crate::util::{close_paren, comma, open_paren, opt_space, required_space, semi_colon};
use crate::value::{value, Value};
use crate::{column_name, column_name_list, ColumnName, State, Stream};
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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum LogicalOperator {
    And,
    Or,
    Not,
}

fn logical_operator<'a>(s: &mut Stream<'a>) -> PResult<LogicalOperator> {
    let _ = required_space.parse_next(s)?;
    let op = alt((
        "AND".map(|_| LogicalOperator::And),
        "OR".map(|_| LogicalOperator::Or),
        "NOT".map(|_| LogicalOperator::Not),
    ))
    .parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    Ok(op)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    /// Less than
    LT,
    ///  Less than or equal to
    LE,
    /// Greater than
    GT,
    /// Greater than or equal to
    GE,
    ///
    Is,
    In,
    Like,
    Glob,
    Exists,
}

fn comparison_operator<'a>(s: &mut Stream<'a>) -> PResult<ComparisonOperator> {
    alt((
        "==".map(|_| ComparisonOperator::Equal),
        "!=".map(|_| ComparisonOperator::NotEqual),
        "<".map(|_| ComparisonOperator::LT),
        "<=".map(|_| ComparisonOperator::LE),
        ">".map(|_| ComparisonOperator::GT),
        ">=".map(|_| ComparisonOperator::GE),
    ))
    .parse_next(s)
}

/// age < 50
/// name == 'Bob'
/// city == 'New York'
/// TODO: note in documentation that we require rhs to be a column name and lhs to be a value. This breaks compabitlibty with sqlite sql syntax but is way easier to parse
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SimpleCondition {
    pub(crate) rhs: ColumnName,
    pub(crate) op: ComparisonOperator,
    pub(crate) lhs: Value,
}

fn simple_condition<'a>(s: &mut Stream<'a>) -> PResult<SimpleCondition> {
    let column_name = column_name.parse_next(s)?;
    let _ = opt_space.parse_next(s)?;
    let op = comparison_operator.parse_next(s)?;
    let _ = opt_space.parse_next(s)?;

    // Get the type from the column's name, then parse the value as that type
    let state: &mut State = unsafe { &mut *s.state };
    let Some(table_name) = &state.condition_table_tracker else {
        panic!("TODO: return err instead of panic");
    };
    let Some(ttype) = state.get_column_type(table_name, &column_name) else {
        panic!("TODO: return Err instead of panic");
    };
    let value = value(*ttype).parse_next(s)?;

    Ok(SimpleCondition {
        rhs: column_name,
        op,
        lhs: value,
    })
}

fn parenthesis_condition<'a>(s: &mut Stream<'a>) -> PResult<ConditionClause> {
    delimited(open_paren, conditions, close_paren).parse_next(s)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Condition {
    Simple(SimpleCondition),
    Parenthesis(ConditionClause),
}

fn condition<'a>(s: &mut Stream<'a>) -> PResult<Condition> {
    alt((
        parenthesis_condition.map(|c| Condition::Parenthesis(c)),
        simple_condition.map(|c| Condition::Simple(c)),
    ))
    .parse_next(s)
}

/// age < 50 AND name == 'Bob'
/// name == 'Bob' AND job == 'Engineer'
/// city == 'New York' OR (age > 100 and job == 'retired')
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConditionClause {
    /// Vector of simple conditions that are separated
    /// The len of this should always be the same as self.separators.len() + 1
    pub(crate) parts: Vec<Condition>,
    /// Logical separators that
    pub(crate) separators: Vec<LogicalOperator>,
}

fn condition_logical_op<'a>(s: &mut Stream<'a>) -> PResult<(Condition, Option<LogicalOperator>)> {
    let condition = condition.parse_next(s)?;
    let op = opt(logical_operator).parse_next(s)?;
    Ok((condition, op))
}

fn conditions<'a>(s: &mut Stream<'a>) -> PResult<ConditionClause> {
    let mut parts = Vec::new();
    let mut separators = Vec::new();

    let foo: Vec<(Condition, Option<LogicalOperator>)> =
        repeat(1.., condition_logical_op).parse_next(s)?;
    for (part, sep) in foo {
        parts.push(part);
        if let Some(sep) = sep {
            separators.push(sep);
        }
    }
    assert!(parts.len() == separators.len() + 1);

    Ok(ConditionClause { parts, separators })
}

fn where_keyword<'a>(s: &mut Stream<'a>) -> PResult<()> {
    let _ = required_space.parse_next(s)?;
    let _ = "WHERE".parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    Ok(())
}

pub fn condition_clause<'a>(s: &mut Stream<'a>) -> PResult<ConditionClause> {
    let _ = where_keyword.parse_next(s)?;
    conditions.parse_next(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        r#type::{DataType, IntegerType},
        value::IntegerValue,
        TableName,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn test_where_keyword() {
        let input_str = " WHERE ";

        let mut state = Box::new(State::new());
        let state_ptr: *mut State = state.as_mut();
        let mut input = Stream {
            input: input_str,
            state: state_ptr,
        };

        let expected = Ok(());
        let actual = where_keyword.parse_next(&mut input);

        assert_eq!(expected, actual);
    }

    #[test]
    fn test_condition_clause() {
        let input_str = " WHERE age > 21";

        let mut state = Box::new(State::new());
        let test_table = TableName::from_str("test");
        state.add_table(test_table.clone());
        state
            .add_column_type(
                &test_table,
                ColumnName::from_str("age"),
                DataType::Integer(IntegerType::Auto),
            )
            .unwrap();
        state.set_current_condition_table(test_table);

        let state_ptr: *mut State = state.as_mut();
        let mut input = Stream {
            input: input_str,
            state: state_ptr,
        };

        let expected = Ok(ConditionClause {
            parts: vec![Condition::Simple(SimpleCondition {
                rhs: ColumnName::from_str("age"),
                op: ComparisonOperator::GT,
                lhs: Value::Integer(IntegerValue::Auto(21)),
            })],
            separators: Vec::new(),
        });
        let actual = condition_clause.parse_next(&mut input);

        assert_eq!(expected, actual);
    }

    #[test]
    fn test_condition_clause_and() {
        let input_str = " WHERE age > 21 AND city == 'SLC'";

        let mut state = Box::new(State::new());
        let test_table = TableName::from_str("test");
        state.add_table(test_table.clone());
        state
            .add_column_type(
                &test_table,
                ColumnName::from_str("age"),
                DataType::Integer(IntegerType::Auto),
            )
            .unwrap();
        state
            .add_column_type(&test_table, ColumnName::from_str("city"), DataType::String)
            .unwrap();
        state.set_current_condition_table(test_table);

        let state_ptr: *mut State = state.as_mut();
        let mut input = Stream {
            input: input_str,
            state: state_ptr,
        };

        let expected = Ok(ConditionClause {
            parts: vec![
                Condition::Simple(SimpleCondition {
                    rhs: ColumnName::from_str("age"),
                    op: ComparisonOperator::GT,
                    lhs: Value::Integer(IntegerValue::Auto(21)),
                }),
                Condition::Simple(SimpleCondition {
                    rhs: ColumnName::from_str("city"),
                    op: ComparisonOperator::Equal,
                    lhs: Value::String(String::from("SLC")),
                }),
            ],
            separators: vec![LogicalOperator::And],
        });
        let actual = condition_clause.parse_next(&mut input);

        assert_eq!(expected, actual);
    }

    #[test]
    fn test_condition_clause_parens() {
        let input_str = " WHERE age > 21 AND (city == 'SLC' OR city == 'New York')";

        let mut state = Box::new(State::new());
        let test_table = TableName::from_str("test");
        state.add_table(test_table.clone());
        state
            .add_column_type(
                &test_table,
                ColumnName::from_str("age"),
                DataType::Integer(IntegerType::Auto),
            )
            .unwrap();
        state
            .add_column_type(&test_table, ColumnName::from_str("city"), DataType::String)
            .unwrap();
        state.set_current_condition_table(test_table);

        let state_ptr: *mut State = state.as_mut();
        let mut input = Stream {
            input: input_str,
            state: state_ptr,
        };

        let expected = Ok(ConditionClause {
            parts: vec![
                Condition::Simple(SimpleCondition {
                    rhs: ColumnName::from_str("age"),
                    op: ComparisonOperator::GT,
                    lhs: Value::Integer(IntegerValue::Auto(21)),
                }),
                Condition::Parenthesis(ConditionClause {
                    parts: vec![
                        Condition::Simple(SimpleCondition {
                            rhs: ColumnName::from_str("city"),
                            op: ComparisonOperator::Equal,
                            lhs: Value::String(String::from("SLC")),
                        }),
                        Condition::Simple(SimpleCondition {
                            rhs: ColumnName::from_str("city"),
                            op: ComparisonOperator::Equal,
                            lhs: Value::String(String::from("New York")),
                        }),
                    ],
                    separators: vec![LogicalOperator::Or],
                }),
            ],
            separators: vec![LogicalOperator::And],
        });
        let actual = condition_clause.parse_next(&mut input);

        assert_eq!(expected, actual);
    }
}
