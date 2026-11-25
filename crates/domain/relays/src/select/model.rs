use std::collections::BTreeMap;

use coracle_lib::filters::Filter;
use serde::Deserialize;
use serde_json::Value;

/// A SELECT: the events matching `filter`, grouped and aggregated as `columns` say,
/// with `options` narrowing the events, ordering the rows and asking for samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub filter: Filter,
    pub columns: Vec<Column>,
    pub options: Options,
}

/// One column of a SELECT's rows: a JMESPath expression whose values split events
/// into groups, or an aggregate of an expression over each group's events.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ColumnWire")]
pub enum Column {
    Group(String),
    Aggregate(Aggregate, String),
}

/// How an aggregate column combines a group's values: `count` counts the events
/// with a value, `sum` adds the integers, `min` and `max` take the extreme number,
/// and `distinct` counts the different values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    Count,
    Sum,
    Min,
    Max,
    Distinct,
}

/// What narrows a SELECT and shapes its answer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "OptionsWire")]
pub struct Options {
    /// Only events for which this expression is truthy count.
    pub r#where: Option<String>,
    /// Only the newest event for each value of this expression counts.
    pub latest: Option<String>,
    /// Rows are sorted by this column.
    pub order: Option<Order>,
    /// At most this many rows are returned, after sorting.
    pub limit: Option<usize>,
    /// Each count, sum and distinct column comes with a sample of its items.
    pub sample: Option<Sample>,
}

/// The column rows are sorted by, counting from 0, and the direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Order {
    pub column: usize,
    pub descending: bool,
}

/// The salt items are ranked by, and how many of the lowest-ranked to return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub salt: String,
    pub size: usize,
}

/// A SELECT's answer: rows holding column values in column order, and when samples
/// were asked for, one entry per row with a sample for each count, sum and distinct
/// column and `None` for the other columns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub rows: Vec<Vec<Value>>,
    pub samples: Option<Vec<Vec<Option<Vec<Value>>>>>,
}

impl Query {
    /// A query, if its columns and options agree with each other.
    pub fn new(filter: Filter, columns: Vec<Column>, options: Options) -> Result<Self, String> {
        if columns.is_empty() {
            return Err("a SELECT needs at least one column".to_string());
        }

        if let Some(order) = options.order
            && order.column >= columns.len()
        {
            return Err(format!(
                "order names column {}, which doesn't exist",
                order.column
            ));
        }

        Ok(Self {
            filter,
            columns,
            options,
        })
    }
}

impl Aggregate {
    /// Whether a sample can stand for this aggregate's items.
    pub fn is_sampled(self) -> bool {
        matches!(
            self,
            Aggregate::Count | Aggregate::Sum | Aggregate::Distinct
        )
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ColumnWire {
    Group(String),
    Aggregate(BTreeMap<String, String>),
}

impl TryFrom<ColumnWire> for Column {
    type Error = String;

    fn try_from(wire: ColumnWire) -> Result<Self, String> {
        let entries = match wire {
            ColumnWire::Group(expression) => return Ok(Column::Group(expression)),
            ColumnWire::Aggregate(entries) => entries,
        };
        let mut entries = entries.into_iter();
        let (Some((name, expression)), None) = (entries.next(), entries.next()) else {
            return Err("an aggregate column has exactly one key".to_string());
        };
        let aggregate = match name.as_str() {
            "count" => Aggregate::Count,
            "sum" => Aggregate::Sum,
            "min" => Aggregate::Min,
            "max" => Aggregate::Max,
            "distinct" => Aggregate::Distinct,
            other => return Err(format!("unknown aggregate {other}")),
        };

        Ok(Column::Aggregate(aggregate, expression))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionsWire {
    r#where: Option<String>,
    latest: Option<String>,
    order: Option<(usize, String)>,
    limit: Option<usize>,
    salt: Option<String>,
    sample: Option<usize>,
}

impl TryFrom<OptionsWire> for Options {
    type Error = String;

    fn try_from(wire: OptionsWire) -> Result<Self, String> {
        let order = match wire.order {
            None => None,
            Some((column, direction)) => Some(Order {
                column,
                descending: match direction.as_str() {
                    "asc" => false,
                    "desc" => true,
                    other => return Err(format!("order is asc or desc, not {other}")),
                },
            }),
        };
        let sample = match (wire.salt, wire.sample) {
            (None, None) => None,
            (Some(salt), Some(size)) if size > 0 => Some(Sample { salt, size }),
            _ => return Err("a sample needs a salt and a size above 0".to_string()),
        };

        Ok(Options {
            r#where: wire.r#where,
            latest: wire.latest,
            order,
            limit: wire.limit,
            sample,
        })
    }
}
