use eyre::{bail, eyre, Result};
use ident::{ident, Ident};
use jiff::fmt::serde::timestamp::microsecond::required;
use jiff::Zoned;
use log::debug;
use r#type::IntegerType;
use r#type::UnsignedIntegerType;
use r#type::{data_type, DataType};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use util::equal_sign;
use util::{close_paren, comma, open_paren, opt_space, required_space, semi_colon};
use value::value;
use value::IntegerValue;
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

mod ident;
mod r#type;
mod util;
mod value;
use value::Value;
mod condition;
use condition::{condition_clause, ConditionClause};

#[derive(Debug)]
pub struct InsertColumnTracker {
    table_name: TableName,
    column_names: Vec<ColumnName>,
    idx: usize,
}

impl InsertColumnTracker {
    pub fn new(table_name: TableName) -> Self {
        Self {
            table_name,
            column_names: Vec::new(),
            idx: 0,
        }
    }

    pub fn add_column(&mut self, name: ColumnName) {
        self.column_names.push(name);
    }

    pub fn get_current_column_name(&self) -> &ColumnName {
        self.column_names.get(self.idx).unwrap() // TODO: handle None
    }

    pub fn next(&mut self) -> &ColumnName {
        let name = self.column_names.get(self.idx).unwrap();
        self.idx += 1;
        if self.idx >= self.column_names.len() {
            self.idx = 0;
        }
        name
    }
}

use indexmap::IndexMap;
#[derive(Debug)]
pub struct State {
    /// Map table name to a map of column name to column type
    table_column_types: HashMap<TableName, IndexMap<ColumnName, DataType>>,

    /// Used to keep track of what column (to get its type) we are working with while inserting
    insert_helper: Option<InsertColumnTracker>, // TODO: this should be a stack for nested statements

    /// Used to keep track of what table we are filtering with a condition
    condition_table_tracker: Option<TableName>, // TODO: this should be a stack for nested statements
}

impl State {
    pub fn new() -> Self {
        Self {
            table_column_types: HashMap::new(),
            insert_helper: None,
            condition_table_tracker: None,
        }
    }

    pub fn existing_table_names(&self) -> impl Iterator<Item = &TableName> {
        self.table_column_types.keys()
    }

    pub fn setup_insert_helper_with_columns<'a>(
        &mut self,
        table_name: TableName,
        columns: &Option<Vec<ColumnName>>,
    ) {
        let mut helper = InsertColumnTracker::new(table_name.clone());
        match columns {
            Some(columns) => {
                for column_name in columns {
                    helper.column_names.push(column_name.to_owned());
                }
            }
            None => {
                // Use all columns
                let table = self.table_column_types.get(&table_name).unwrap();
                for column_name in table.keys() {
                    helper.column_names.push(column_name.clone());
                }
            }
        }

        self.insert_helper = Some(helper);
    }

    pub fn table_exists(&mut self, table_name: &TableName) -> bool {
        self.table_column_types.contains_key(table_name)
    }

    pub fn remove_table(&mut self, table_name: &TableName) {
        self.table_column_types.remove(table_name);
    }

    pub fn cleanup_insert_helper(&mut self) {
        self.insert_helper = None;
    }

    pub fn set_current_condition_table(&mut self, table_name: TableName) {
        self.condition_table_tracker = Some(table_name);
    }

    pub fn reset_current_condition_table(&mut self) {
        self.condition_table_tracker = None;
    }

    pub fn add_table(&mut self, table_name: TableName) {
        self.table_column_types.insert(table_name, IndexMap::new());
    }

    pub fn add_column_type(
        &mut self,
        table_name: &TableName,
        column_name: ColumnName,
        column_type: DataType,
    ) -> Result<()> {
        let Some(inner_map) = self.table_column_types.get_mut(table_name) else {
            bail!("Table '{}' not registered", table_name);
        };
        inner_map.insert(column_name, column_type);
        Ok(())
    }

    pub fn get_column_type(
        &self,
        table_name: &TableName,
        column_name: &ColumnName,
    ) -> Option<&DataType> {
        if let Some(inner_map) = self.table_column_types.get(table_name) {
            if let Some(column_type) = inner_map.get(column_name) {
                return Some(column_type);
            }
        }
        None
    }
}

type Stream<'a> = Stateful<&'a str, *mut State>;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
pub struct TableName(Ident);
impl TableName {
    pub fn from_str(s: &str) -> Self {
        Self {
            0: Ident::from_str(s),
        }
    }
}
impl fmt::Display for TableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn table_name<'a>(s: &mut Stream<'a>) -> PResult<TableName> {
    ident.map(|name| TableName { 0: name }).parse_next(s)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
pub struct ColumnName(Ident);
impl ColumnName {
    pub fn from_str(s: &str) -> Self {
        Self {
            0: Ident::from_str(s),
        }
    }
}

impl fmt::Display for ColumnName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn column_name<'a>(s: &mut Stream<'a>) -> PResult<ColumnName> {
    ident.map(|name| ColumnName { 0: name }).parse_next(s)
}

use bitflags::bitflags;
bitflags! {
    #[derive(Debug,PartialEq, Eq, PartialOrd, Ord)]
    pub struct ColumnAttributes: u8 {
        const NONE = 0b0000;
        const PRIMARY_KEY = 0b0001;
        const AUTOINCREMENT = 0b0100;
        const NOT_NULL = 0b0100;
        const UNIQUE = 0b1000;
    }
}

fn primary_key_attribute<'a>(s: &mut Stream<'a>) -> PResult<ColumnAttributes> {
    let _ = alt(("PRIMARY", "primary", "Primary")).parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    let _ = alt(("KEY", "key", "Key")).parse_next(s)?;
    Ok(ColumnAttributes::PRIMARY_KEY)
}

fn auto_increment_attribute<'a>(s: &mut Stream<'a>) -> PResult<ColumnAttributes> {
    let _ = alt((
        "AUTOINCREMENT",
        "autoincrement",
        "AUTO_INCREMENT",
        "AutoIncrement",
        "Autoincrement",
        "auto_increment",
        "Auto_Increment",
    ))
    .parse_next(s)?;
    Ok(ColumnAttributes::AUTOINCREMENT)
}

fn not_null_attribute<'a>(s: &mut Stream<'a>) -> PResult<ColumnAttributes> {
    let _ = alt(("NOT", "not", "Not")).parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    let _ = alt(("NULL", "null", "Null")).parse_next(s)?;
    Ok(ColumnAttributes::NOT_NULL)
}

fn unique_attribute<'a>(s: &mut Stream<'a>) -> PResult<ColumnAttributes> {
    let _ = alt(("UNIQUE", "unique", "Unique")).parse_next(s)?;
    Ok(ColumnAttributes::UNIQUE)
}

fn attribute<'a>(s: &mut Stream<'a>) -> PResult<ColumnAttributes> {
    alt((
        primary_key_attribute,
        auto_increment_attribute,
        not_null_attribute,
        unique_attribute,
    ))
    .parse_next(s)
}

fn column_attributes<'a>(s: &mut Stream<'a>) -> PResult<ColumnAttributes> {
    let _ = required_space.parse_next(s)?;
    let atts_vec: Vec<ColumnAttributes> =
        separated(0.., attribute, required_space).parse_next(s)?;
    let mut attributes = ColumnAttributes::NONE;
    for a in atts_vec {
        attributes = attributes | a;
    }
    Ok(attributes)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DefaultStrat {
    Function(String), // TODO
    Value(String),    // TODO
}

fn default_function<'a>(s: &mut Stream<'a>) -> PResult<String> {
    todo!();
}

fn default_value<'a>(s: &mut Stream<'a>) -> PResult<String> {
    todo!();
}

fn default_strat<'a>(s: &mut Stream<'a>) -> PResult<DefaultStrat> {
    let _ = required_space.parse_next(s)?;
    let _ = alt(("DEFAULT", "default")).parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    let strat = alt((
        default_function.map(|f| DefaultStrat::Function(f)),
        default_value.map(|v| DefaultStrat::Value(v)),
    ))
    .parse_next(s)?;
    Ok(strat)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ColumnDecleration {
    name: ColumnName,
    ttype: DataType,
    attributes: ColumnAttributes,
    default: Option<DefaultStrat>,
}

fn column_decleration<'a>(s: &mut Stream<'a>) -> PResult<ColumnDecleration> {
    let name = column_name.parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    let ttype = data_type.parse_next(s)?;
    let attributes = match opt(column_attributes).parse_next(s)? {
        Some(attributes) => attributes,
        None => ColumnAttributes::empty(),
    };
    let default = opt(default_strat).parse_next(s)?;
    Ok(ColumnDecleration {
        name,
        ttype,
        attributes,
        default,
    })
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Schema {
    columns: Vec<ColumnDecleration>,
}

fn schema<'a>(s: &mut Stream<'a>) -> PResult<Schema> {
    let _ = open_paren.parse_next(s)?;
    let columns = separated(1.., column_decleration, comma).parse_next(s)?;
    let _ = close_paren.parse_next(s)?;
    Ok(Schema { columns })
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CreateTableStatement {
    table_name: TableName,
    schema: Schema,
}

fn create_table_statement<'a>(s: &mut Stream<'a>) -> PResult<CreateTableStatement> {
    let table_name = table_name.parse_next(s)?;
    let _ = opt_space.parse_next(s)?;
    let schema = schema.parse_next(s)?;

    // Register the new table and its schema in our state.
    // This will be used later to figure out what type to parse inserted values as
    let state: &mut State = unsafe { &mut *s.state };
    state.add_table(table_name.to_owned());
    for column in &schema.columns {
        let column_name = &column.name;
        let column_type = column.ttype;
        state
            .add_column_type(&table_name, column_name.to_owned(), column_type)
            .unwrap(); // TODO: handle err
    }

    Ok(CreateTableStatement { table_name, schema })
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AlterTableStatement {
    table_name: TableName,
}

fn alter_table_statement<'a>(s: &mut Stream<'a>) -> PResult<AlterTableStatement> {
    todo!();
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DropTableStatement {
    table_name: TableName,
}

pub fn drop_table_statement<'a>(s: &mut Stream<'a>) -> PResult<DropTableStatement> {
    let table_name = table_name.parse_next(s)?;

    // Verify table exists
    let state: &mut State = unsafe { &mut *s.state };
    if !state.table_exists(&table_name) {
        cut_err(fail)
            .context(StrContext::Label("table name"))
            .context(StrContext::Expected(StrContextValue::Description(
                "registered table name",
            )))
            .parse_next(s)?;
    }
    // Remove table so trying to access it in the future will result in an error
    state.remove_table(&table_name);

    Ok(DropTableStatement { table_name })
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TruncateStatement {
    table_name: TableName,
}

pub fn truncate_table_statement<'a>(s: &mut Stream<'a>) -> PResult<TruncateStatement> {
    table_name
        .map(|table_name| TruncateStatement { table_name })
        .parse_next(s)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommitStatement {}

pub fn commit_statement<'a>(s: &mut Stream<'a>) -> PResult<CommitStatement> {
    todo!();
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RollbackStatement {}

pub fn rollback_statement<'a>(s: &mut Stream<'a>) -> PResult<RollbackStatement> {
    todo!();
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SelectedColumns {
    All,
    Specified(Vec<ColumnName>),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SelectStatement {
    columns: SelectedColumns,
    table_name: TableName,
    condition: Option<ConditionClause>,
}

fn selected_columns<'a>(s: &mut Stream<'a>) -> PResult<SelectedColumns> {
    alt((
        "*".map(|_| SelectedColumns::All),
        column_name_list.map(|list| SelectedColumns::Specified(list)),
    ))
    .parse_next(s)
}

fn from_keyword<'a>(s: &mut Stream<'a>) -> PResult<()> {
    let _ = required_space.parse_next(s)?;
    let _ = "FROM".parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    Ok(())
}

pub fn select_statement<'a>(s: &mut Stream<'a>) -> PResult<SelectStatement> {
    let columns = selected_columns.parse_next(s)?;
    let _ = from_keyword.parse_next(s)?;
    let table_name = table_name.parse_next(s)?;

    // Save the name of the table we are working with for use to parse the type of values in conditions
    let state: &mut State = unsafe { &mut *s.state };
    state.set_current_condition_table(table_name.clone());
    let condition = opt(condition_clause).parse_next(s)?;
    state.reset_current_condition_table();

    Ok(SelectStatement {
        columns,
        table_name,
        condition,
    })
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct InsertIntoStatement {
    table_name: TableName,
    columns: Option<Vec<ColumnName>>,
    values: Vec<Vec<Value>>,
}

fn column_name_list<'a>(s: &mut Stream<'a>) -> PResult<Vec<ColumnName>> {
    separated(1.., column_name, comma).parse_next(s)
}

fn column_names<'a>(s: &mut Stream<'a>) -> PResult<Vec<ColumnName>> {
    delimited(open_paren, column_name_list, close_paren).parse_next(s)
}

fn insert_value<'a>(s: &mut Stream<'a>) -> PResult<Value> {
    // Get the type of the column we are about to parse
    let state: &mut State = unsafe { &mut *s.state };
    let Some(ref helper) = state.insert_helper else {
        panic!("TODO: don't panic, handle err");
    };
    let table_name = &helper.table_name;
    let column_name = helper.get_current_column_name();
    let Some(ttype) = state.get_column_type(&table_name, &column_name) else {
        panic!("TODO: don't panic, handle err");
    };

    // Parse the specified type
    let value = value(*ttype).parse_next(s)?;

    // Move insert helper onto the column after this one is processed
    let Some(ref mut helper) = state.insert_helper.as_mut() else {
        panic!("TODO: dont panic, but return err instead");
    };
    helper.next();

    Ok(value)
}

fn value_list<'a>(s: &mut Stream<'a>) -> PResult<Vec<Value>> {
    separated(1.., insert_value, comma).parse_next(s)
}

fn single_insert_row<'a>(s: &mut Stream<'a>) -> PResult<Vec<Value>> {
    delimited(open_paren, value_list, close_paren).parse_next(s)
}

fn multiple_insert_rows<'a>(s: &mut Stream<'a>) -> PResult<Vec<Vec<Value>>> {
    separated(1.., single_insert_row, comma).parse_next(s)
}

pub fn insert_into_statement<'a>(s: &mut Stream<'a>) -> PResult<InsertIntoStatement> {
    let table_name = table_name.parse_next(s)?;

    let columns = opt(column_names).parse_next(s)?;
    let _ = alt(("VALUES", "values", "Values")).parse_next(s)?;

    // Parse the column values using our state to keep track of the column's types
    let state: &mut State = unsafe { &mut *s.state };
    state.setup_insert_helper_with_columns(table_name.to_owned(), &columns);
    let values = multiple_insert_rows.parse_next(s)?;
    state.cleanup_insert_helper();

    Ok(InsertIntoStatement {
        table_name,
        columns,
        values,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct UpdateStatement {
    table_name: TableName,
    column_value_pairs: HashMap<ColumnName, Value>,
    condition: Option<ConditionClause>,
}

fn set_keyword<'a>(s: &mut Stream<'a>) -> PResult<()> {
    let _ = required_space.parse_next(s)?;
    let _ = "SET".parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    Ok(())
}

fn name_value_assignment<'a>(s: &mut Stream<'a>) -> PResult<(ColumnName, Value)> {
    let column_name = column_name.parse_next(s)?;
    let _ = equal_sign.parse_next(s)?;

    // Get the type from the column's name, then parse the value as that type
    let state: &mut State = unsafe { &mut *s.state };
    let Some(table_name) = &state.condition_table_tracker else {
        panic!("Unable to get table"); // TODO: return err instead of panic
    };
    let Some(ttype) = state.get_column_type(table_name, &column_name) else {
        panic!(
            "Unable to get column type for table: {}, column: {}",
            table_name, column_name
        ); // TODO: return err instead of panic
    };
    let value = value(*ttype).parse_next(s)?;

    Ok((column_name, value))
}

fn column_value_pairs<'a>(s: &mut Stream<'a>) -> PResult<HashMap<ColumnName, Value>> {
    let mut map = HashMap::new();
    let kv_pairs: Vec<(ColumnName, Value)> =
        separated(1.., name_value_assignment, comma).parse_next(s)?;
    for (name, value) in kv_pairs {
        map.insert(name, value);
    }
    Ok(map)
}

pub fn update_statement<'a>(s: &mut Stream<'a>) -> PResult<UpdateStatement> {
    let table_name = table_name.parse_next(s)?;
    let _ = set_keyword.parse_next(s)?;

    // Save the name of the table we are working with for use to parse the type of values in conditions and column_value_pairs
    let state: &mut State = unsafe { &mut *s.state };
    state.set_current_condition_table(table_name.clone());
    let column_value_pairs = column_value_pairs.parse_next(s)?;
    let condition = opt(condition_clause).parse_next(s)?;
    state.reset_current_condition_table();

    Ok(UpdateStatement {
        table_name,
        column_value_pairs,
        condition,
    })
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeleteFromStatement {
    table_name: TableName,
    condition: Option<ConditionClause>,
}

pub fn delete_from_statement<'a>(s: &mut Stream<'a>) -> PResult<DeleteFromStatement> {
    let table_name = table_name.parse_next(s)?;

    // Save the name of the table we are working with for use to parse the type of values in conditions
    let state: &mut State = unsafe { &mut *s.state };
    state.set_current_condition_table(table_name.clone());
    let condition = opt(condition_clause).parse_next(s)?;
    state.reset_current_condition_table();

    Ok(DeleteFromStatement {
        table_name,
        condition,
    })
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BeginTransactionStatement;

pub fn begin_transaction_statement<'a>(s: &mut Stream<'a>) -> PResult<BeginTransactionStatement> {
    todo!();
}

#[derive(Debug, Eq, PartialEq)]
pub enum Statement {
    Create(CreateTableStatement),
    Drop(DropTableStatement),
    Truncate(TruncateStatement),
    Commit(CommitStatement),
    Rollback(RollbackStatement),
    Select(SelectStatement),
    Insert(InsertIntoStatement),
    Update(UpdateStatement),
    Delete(DeleteFromStatement),
    Begin(BeginTransactionStatement),
    Alter(AlterTableStatement),
}

fn create<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    create_table_statement
        .map(|s| Statement::Create(s))
        .parse_next(s)
}

fn drop<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    drop_table_statement
        .map(|s| Statement::Drop(s))
        .parse_next(s)
}

fn truncate<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    truncate_table_statement
        .map(|s| Statement::Truncate(s))
        .parse_next(s)
}

fn commit<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    commit_statement.map(|s| Statement::Commit(s)).parse_next(s)
}

fn rollback<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    rollback_statement
        .map(|s| Statement::Rollback(s))
        .parse_next(s)
}

fn select<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    select_statement.map(|s| Statement::Select(s)).parse_next(s)
}

fn insert<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    insert_into_statement
        .map(|s| Statement::Insert(s))
        .parse_next(s)
}

fn update<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    update_statement.map(|s| Statement::Update(s)).parse_next(s)
}

fn delete<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    delete_from_statement
        .map(|s| Statement::Delete(s))
        .parse_next(s)
}

fn begin<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    begin_transaction_statement
        .map(|s| Statement::Begin(s))
        .parse_next(s)
}

fn alter<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    alter_table_statement
        .map(|s| Statement::Alter(s))
        .parse_next(s)
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
enum Keyword {
    CreateTable,
    InsertInto,
    DropTable,
    TruncateTable,
    AlterTable,
    DeleteFrom,
    Select,
    Update,
}

fn keyword<'a>(s: &mut Stream<'a>) -> PResult<Keyword> {
    // TODO: make keywords case insensative
    let keyword = alt((
        "CREATE TABLE".map(|_| Keyword::CreateTable), // TODO: only match 'create' then in that function match create subcommand like table, view, index, etc
        "INSERT INTO".map(|_| Keyword::InsertInto),
        "DROP TABLE".map(|_| Keyword::DropTable),
        "TRUNCATE TABLE".map(|_| Keyword::TruncateTable),
        "ALTER TABLE".map(|_| Keyword::AlterTable),
        "DELETE FROM".map(|_| Keyword::DeleteFrom),
        "SELECT".map(|_| Keyword::Select),
        "UPDATE".map(|_| Keyword::Update),
    ))
    .parse_next(s)?;
    let _ = required_space.parse_next(s)?;
    Ok(keyword)
}

fn statement<'a>(s: &mut Stream<'a>) -> PResult<Statement> {
    let statement = match keyword.parse_next(s)? {
        Keyword::CreateTable => create.parse_next(s)?,
        Keyword::InsertInto => insert.parse_next(s)?,
        Keyword::DropTable => drop.parse_next(s)?,
        Keyword::TruncateTable => truncate.parse_next(s)?,
        Keyword::AlterTable => alter.parse_next(s)?,
        Keyword::DeleteFrom => delete.parse_next(s)?,
        Keyword::Select => select.parse_next(s)?,
        Keyword::Update => update.parse_next(s)?,
    };
    Ok(statement)
}

fn statements<'a>(s: &mut Stream<'a>) -> PResult<Vec<Statement>> {
    let statements = separated(1.., statement, semi_colon).parse_next(s)?;
    let _ = opt(semi_colon).parse_next(s)?;
    Ok(statements)
}

#[derive(Debug)]
pub struct StatementError {
    message: String,
    span: std::ops::Range<usize>,
    input: String,
}

impl StatementError {
    /// Taken from https://docs.rs/winnow/latest/winnow/_tutorial/chapter_7/index.html
    fn from_parse<'a>(error: ParseError<Stream<'a>, ContextError>, input: String) -> Self {
        let message = error.inner().to_string();
        let start = error.offset();
        let end = (start + 1..)
            .find(|e| input.is_char_boundary(*e))
            .unwrap_or(start);
        Self {
            message,
            span: start..end,
            input,
        }
    }
}

impl fmt::Display for StatementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = annotate_snippets::Level::Error
            .title(&self.message)
            .snippet(
                annotate_snippets::Snippet::source(&self.input)
                    .fold(true)
                    .annotation(annotate_snippets::Level::Error.span(self.span.clone())),
            );
        let renderer = annotate_snippets::Renderer::plain();
        let rendered = renderer.render(message);
        rendered.fmt(f)
    }
}

impl std::error::Error for StatementError {}

#[derive(Debug)]
pub struct Settings {}
impl Default for Settings {
    fn default() -> Self {
        Self {}
    }
}

#[derive(Debug)]
pub struct SQLEngine {
    settings: Settings,
    state: Box<State>,
}

impl SQLEngine {
    pub fn new() -> Self {
        Self {
            settings: Settings::default(),
            state: Box::new(State::new()),
        }
    }
    pub fn with_settings(settings: Settings) -> Self {
        let state = Box::new(State::new());
        Self { settings, state }
    }

    pub fn init_tables(&mut self, tables: HashMap<TableName, IndexMap<ColumnName, DataType>>) {
        for (table_name, inner_map) in tables {
            self.state.add_table(table_name.clone());
            for (column_name, column_type) in inner_map {
                self.state
                    .add_column_type(&table_name, column_name, column_type)
                    .unwrap();
            }
        }
    }

    pub fn eval(&mut self, sql: &str) -> Result<Vec<Statement>, StatementError> {
        // TODO: parse multiple statements separated by ';'

        // TODO: instead of using a top level 'alt(())' parser, manually parse the statement type ('create table', 'insert into', 'drop table', etc).
        // This will allow for better error messages and will stop the parser from trying all the alt combos when its obviously not one of the other cases
        // let ttype = parse_type(input);
        // match ttype {
        //    "CREATE TABLE" => ...,
        //    "INSERT INTO" => ...,
        //}
        // Actually dont do this! Use winnow's dispatch macro!

        /*
        // Testing code: TODO remove
        let mut map = HashMap::new();
        let mut inner = IndexMap::new();
        inner.insert(OwnedIdent::from_str("name"), DataType::String);
        inner.insert(
            OwnedIdent::from_str("age"),
            DataType::Integer(IntegerType::Unsigned(UnsignedIntegerType::U16)),
        );
        map.insert(OwnedIdent::from_str("people"), inner);
        self.init_tables(map);
        */

        let state_ptr: *mut State = self.state.as_mut(); // TODO: now that Ident is owned, maybe this doesn't need to be a ptr but a ref will do
        let input = Stream {
            input: sql,
            state: state_ptr,
        };

        statements
            .parse(input)
            .map_err(|e| StatementError::from_parse(e, sql.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use condition::{Condition, SimpleCondition};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_create() {
        let input = "CREATE TABLE table(id u32 PRIMARY KEY AUTOINCREMENT, name STRING NOT NULL);";
        let expected = vec![Statement::Create(CreateTableStatement {
            table_name: TableName::from_str("table"),
            schema: Schema {
                columns: vec![
                    ColumnDecleration {
                        name: ColumnName::from_str("id"),
                        ttype: DataType::Integer(r#type::IntegerType::Unsigned(
                            UnsignedIntegerType::U32,
                        )),
                        attributes: ColumnAttributes::PRIMARY_KEY | ColumnAttributes::AUTOINCREMENT,
                        default: None,
                    },
                    ColumnDecleration {
                        name: ColumnName::from_str("name"),
                        ttype: DataType::String,
                        attributes: ColumnAttributes::NOT_NULL,
                        default: None,
                    },
                ],
            },
        })];
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_insert() {
        let input = "CREATE TABLE people(id INTEGER PRIMARY KEY AUTOINCREMENT, name STRING NOT NULL, age INTEGER);
        INSERT INTO people(name, age) VALUES ('Adam', 26), ('Bob', 27);";
        let expected = vec![
            Statement::Create(CreateTableStatement {
                table_name: TableName::from_str("people"),
                schema: Schema {
                    columns: vec![
                        ColumnDecleration {
                            name: ColumnName::from_str("id"),
                            ttype: DataType::Integer(IntegerType::Auto),
                            attributes: ColumnAttributes::PRIMARY_KEY
                                | ColumnAttributes::AUTOINCREMENT,
                            default: None,
                        },
                        ColumnDecleration {
                            name: ColumnName::from_str("name"),
                            ttype: DataType::String,
                            attributes: ColumnAttributes::NOT_NULL,
                            default: None,
                        },
                        ColumnDecleration {
                            name: ColumnName::from_str("age"),
                            ttype: DataType::Integer(IntegerType::Auto),
                            attributes: ColumnAttributes::NONE,
                            default: None,
                        },
                    ],
                },
            }),
            Statement::Insert(InsertIntoStatement {
                table_name: TableName::from_str("people"),
                columns: Some(vec![
                    ColumnName::from_str("name"),
                    ColumnName::from_str("age"),
                ]),
                values: vec![
                    vec![
                        Value::String("Adam".to_string()),
                        Value::Integer(IntegerValue::Auto(26)),
                    ],
                    vec![
                        Value::String("Bob".to_string()),
                        Value::Integer(IntegerValue::Auto(27)),
                    ],
                ],
            }),
        ];
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_drop() {
        let input = "CREATE TABLE test(id INTEGER);\nDROP TABLE test;";
        let expected = Statement::Drop(DropTableStatement {
            table_name: TableName::from_str("test"),
        });
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual[1]);
    }

    #[test]
    fn test_truncate() {
        let input = "CREATE TABLE test(id INTEGER);\nTRUNCATE TABLE test;";
        let expected = Statement::Truncate(TruncateStatement {
            table_name: TableName::from_str("test"),
        });
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual[1]);
    }

    use condition::ComparisonOperator;

    #[test]
    fn test_delete() {
        let input = "CREATE TABLE test(id INTEGER);\nDELETE FROM test WHERE id > 10;";
        let expected = Statement::Delete(DeleteFromStatement {
            table_name: TableName::from_str("test"),
            condition: Some(ConditionClause {
                parts: vec![Condition::Simple(SimpleCondition {
                    rhs: ColumnName::from_str("id"),
                    op: ComparisonOperator::GT,
                    lhs: Value::Integer(IntegerValue::Auto(10)),
                })],
                separators: Vec::new(),
            }),
        });
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual[1]);
    }

    #[test]
    fn test_select_star() {
        let input = "CREATE TABLE test(id INTEGER);\nSELECT * FROM test WHERE id > 10;";
        let expected = Statement::Select(SelectStatement {
            table_name: TableName::from_str("test"),
            columns: SelectedColumns::All,
            condition: Some(ConditionClause {
                parts: vec![Condition::Simple(SimpleCondition {
                    rhs: ColumnName::from_str("id"),
                    op: ComparisonOperator::GT,
                    lhs: Value::Integer(IntegerValue::Auto(10)),
                })],
                separators: Vec::new(),
            }),
        });
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual[1]);
    }

    #[test]
    fn test_select_specific_columns() {
        let input = "CREATE TABLE test(id INTEGER, x INTEGER);\nSELECT id, x FROM test;";
        let expected = Statement::Select(SelectStatement {
            table_name: TableName::from_str("test"),
            columns: SelectedColumns::Specified(vec![
                ColumnName::from_str("id"),
                ColumnName::from_str("x"),
            ]),
            condition: None,
        });
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual[1]);
    }

    #[test]
    fn test_update_set_single_value() {
        let input = "CREATE TABLE test(id INTEGER, x INTEGER);\nUPDATE test SET x = 10;";
        let expected = Statement::Update(UpdateStatement {
            table_name: TableName::from_str("test"),
            condition: None,
            column_value_pairs: HashMap::from([(
                ColumnName::from_str("x"),
                Value::Integer(IntegerValue::Auto(10)),
            )]),
        });
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual[1]);
    }

    #[test]
    fn test_update_set_multiple_values() {
        let input =
            "CREATE TABLE test(id INTEGER, x INTEGER, y INTEGER);\nUPDATE test SET x = 10, y=20;";
        let expected = Statement::Update(UpdateStatement {
            table_name: TableName::from_str("test"),
            condition: None,
            column_value_pairs: HashMap::from([
                (
                    ColumnName::from_str("x"),
                    Value::Integer(IntegerValue::Auto(10)),
                ),
                (
                    ColumnName::from_str("y"),
                    Value::Integer(IntegerValue::Auto(20)),
                ),
            ]),
        });
        let mut sql = SQLEngine::new();
        let actual = sql.eval(&input).unwrap();
        assert_eq!(expected, actual[1]);
    }
}
