use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use jmespath::Rcvar;
use serde_json::{Value, json};

use super::expression::{Expression, MAX_SAFE_INTEGER, is_truthy, normalize, to_variable};
use super::model::{Aggregate, Column, Query, Selection};
use super::sample::{Sampler, rank};
use crate::events::StoredEvent;

/// A query with its expressions compiled, ready to run over events.
pub struct Plan {
    pub query: Query,
    columns: Vec<(Option<Aggregate>, Expression)>,
    r#where: Option<Expression>,
    latest: Option<Expression>,
}

/// What one row has gathered for one aggregate column.
struct Cell {
    count: u64,
    sum: i128,
    min: Option<f64>,
    max: Option<f64>,
    values: HashSet<String>,
    sampler: Option<Sampler>,
}

/// A row's groups, and a cell for each aggregate column.
struct Row {
    groups: Vec<Value>,
    cells: Vec<Option<Cell>>,
}

impl Plan {
    /// Compile a query's expressions.
    pub fn new(query: Query) -> Result<Self, String> {
        let columns = query
            .columns
            .iter()
            .map(|column| match column {
                Column::Group(source) => Ok((None, Expression::compile(source)?)),
                Column::Aggregate(aggregate, source) => {
                    Ok((Some(*aggregate), Expression::compile(source)?))
                }
            })
            .collect::<Result<_, String>>()?;
        let compile =
            |source: &Option<String>| source.as_deref().map(Expression::compile).transpose();

        Ok(Self {
            columns,
            r#where: compile(&query.options.r#where)?,
            latest: compile(&query.options.latest)?,
            query,
        })
    }

    /// The rows the query computes over the events its filter selected.
    pub fn evaluate(&self, events: &[StoredEvent]) -> Selection {
        let sample = self.query.options.sample.as_ref();
        let grouping: Vec<usize> = (0..self.columns.len())
            .filter(|&i| self.columns[i].0.is_none())
            .collect();
        let new_row = |groups: Vec<Value>| Row {
            groups,
            cells: self
                .columns
                .iter()
                .map(|(aggregate, _)| {
                    aggregate.map(|aggregate| Cell {
                        count: 0,
                        sum: 0,
                        min: None,
                        max: None,
                        values: HashSet::new(),
                        sampler: sample
                            .filter(|_| aggregate.is_sampled())
                            .map(|sample| Sampler::new(sample.size)),
                    })
                })
                .collect(),
        };

        let mut rows: BTreeMap<String, Row> = BTreeMap::new();
        if grouping.is_empty() {
            rows.insert(canonical(&json!([])), new_row(Vec::new()));
        }

        for (event, variable) in self.select(events) {
            let id = event.head.id_hex();
            let event_rank = sample.map(|sample| rank(&sample.salt, &event.head.id));
            let values: Vec<Option<Value>> = self
                .columns
                .iter()
                .map(|(aggregate, expression)| aggregate.map(|_| expression.evaluate(&variable)))
                .collect();
            let combos = grouping.iter().fold(vec![Vec::new()], |combos, &i| {
                let groups = distinct(self.columns[i].1.evaluate(&variable));
                combos
                    .iter()
                    .flat_map(|combo| {
                        groups.iter().map(move |group| {
                            let mut next: Vec<Value> = combo.clone();
                            next.push(group.clone());
                            next
                        })
                    })
                    .collect()
            });

            for combo in combos {
                let row = rows
                    .entry(canonical(&Value::Array(combo.clone())))
                    .or_insert_with(|| new_row(combo));

                for (i, (aggregate, _)) in self.columns.iter().enumerate() {
                    let (Some(aggregate), Some(cell), Some(value)) =
                        (aggregate, row.cells[i].as_mut(), &values[i])
                    else {
                        continue;
                    };

                    match aggregate {
                        Aggregate::Count if !value.is_null() => {
                            cell.count += 1;
                            if let (Some(sampler), Some(rank)) = (&mut cell.sampler, event_rank) {
                                sampler.offer(rank, &id, || json!(id));
                            }
                        }
                        Aggregate::Sum => {
                            if let Some(amount) = as_integer(value) {
                                cell.sum += i128::from(amount);
                                if let (Some(sampler), Some(rank), true) =
                                    (&mut cell.sampler, event_rank, amount > 0)
                                {
                                    sampler
                                        .offer(rank / amount as f64, &id, || json!([id, amount]));
                                }
                            }
                        }
                        Aggregate::Min => {
                            if let Some(number) = value.as_f64() {
                                cell.min = Some(cell.min.map_or(number, |min| min.min(number)));
                            }
                        }
                        Aggregate::Max => {
                            if let Some(number) = value.as_f64() {
                                cell.max = Some(cell.max.map_or(number, |max| max.max(number)));
                            }
                        }
                        Aggregate::Distinct => {
                            for element in distinct(value.clone()) {
                                let key = canonical(&element);
                                if element.is_null() || !cell.values.insert(key.clone()) {
                                    continue;
                                }
                                if let (Some(sampler), Some(sample)) = (&mut cell.sampler, sample) {
                                    let rank = rank(&sample.salt, key.as_bytes());
                                    sampler.offer(rank, &key, || json!([element, id]));
                                }
                            }
                        }
                        Aggregate::Count => {}
                    }
                }
            }
        }

        let mut finished: Vec<(Vec<Value>, Vec<Option<Cell>>)> = rows
            .into_values()
            .map(|row| {
                let mut groups = row.groups.into_iter();
                let values = self
                    .columns
                    .iter()
                    .zip(&row.cells)
                    .map(|((aggregate, _), cell)| match (aggregate, cell) {
                        (Some(aggregate), Some(cell)) => value_of(*aggregate, cell),
                        _ => groups.next().unwrap_or(Value::Null),
                    })
                    .collect();
                (values, row.cells)
            })
            .collect();

        if let Some(order) = self.query.options.order {
            finished.sort_by(|(a, _), (b, _)| {
                let (a, b) = (&a[order.column], &b[order.column]);
                match (a.is_null(), b.is_null()) {
                    (true, true) => Ordering::Equal,
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                    _ if order.descending => compare(b, a),
                    _ => compare(a, b),
                }
            });
        }
        if let Some(limit) = self.query.options.limit {
            finished.truncate(limit);
        }

        let (rows, cells): (Vec<_>, Vec<_>) = finished.into_iter().unzip();
        let samples = sample.map(|_| {
            cells
                .into_iter()
                .map(|cells| {
                    cells
                        .into_iter()
                        .map(|cell| cell.and_then(|cell| cell.sampler).map(Sampler::drain))
                        .collect()
                })
                .collect()
        });

        Selection { rows, samples }
    }

    /// The events that pass `where`, with only the newest for each value of `latest`,
    /// each with the variable its expressions read.
    fn select<'a>(&self, events: &'a [StoredEvent]) -> Vec<(&'a StoredEvent, Rcvar)> {
        let passing = events.iter().filter_map(|event| {
            let variable = to_variable(&event.to_json(false))?;
            let keep = self
                .r#where
                .as_ref()
                .is_none_or(|condition| is_truthy(&condition.evaluate(&variable)));
            keep.then_some((event, variable))
        });

        let Some(latest) = &self.latest else {
            return passing.collect();
        };

        let mut newest: HashMap<String, (&StoredEvent, Rcvar)> = HashMap::new();
        for (event, variable) in passing {
            let key = canonical(&latest.evaluate(&variable));
            let is_newer = newest.get(&key).is_none_or(|(current, _)| {
                (event.head.created_at, std::cmp::Reverse(event.head.id))
                    > (current.head.created_at, std::cmp::Reverse(current.head.id))
            });
            if is_newer {
                newest.insert(key, (event, variable));
            }
        }

        newest.into_values().collect()
    }
}

/// A value's RFC 8785 canonical JSON, by which values are equal and distinct values
/// are ranked.
fn canonical(value: &Value) -> String {
    serde_jcs::to_string(value).expect("json values serialize")
}

/// The distinct elements of an array, or the value itself.
fn distinct(value: Value) -> Vec<Value> {
    let Value::Array(values) = value else {
        return vec![value];
    };
    let mut seen = HashSet::new();

    values
        .into_iter()
        .filter(|value| seen.insert(canonical(value)))
        .collect()
}

/// A number that holds an integer within the range JSON numbers keep exact.
fn as_integer(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        let number = value.as_f64()?;
        (number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER).then_some(number as i64)
    })
}

/// What an aggregate column says for a row. Sums too large for a JSON number to hold
/// exactly are written as decimal strings.
fn value_of(aggregate: Aggregate, cell: &Cell) -> Value {
    match aggregate {
        Aggregate::Count => json!(cell.count),
        Aggregate::Sum if (cell.sum.abs() as f64) <= MAX_SAFE_INTEGER => json!(cell.sum as i64),
        Aggregate::Sum => json!(cell.sum.to_string()),
        Aggregate::Min => cell.min.map_or(Value::Null, |min| normalize(json!(min))),
        Aggregate::Max => cell.max.map_or(Value::Null, |max| normalize(json!(max))),
        Aggregate::Distinct => json!(cell.values.len()),
    }
}

/// Numbers before strings before anything else. Numbers, including sums written as
/// strings, sort numerically, and strings by code point.
fn compare(a: &Value, b: &Value) -> Ordering {
    let rank = |value: &Value| match value {
        Value::Number(_) => 0,
        Value::String(text) if text.parse::<i128>().is_ok() => 0,
        Value::String(_) => 1,
        _ => 2,
    };
    let number = |value: &Value| match value {
        Value::String(text) => text.parse::<f64>().unwrap_or(0.0),
        _ => value.as_f64().unwrap_or(0.0),
    };

    match (rank(a), rank(b)) {
        (0, 0) => number(a).total_cmp(&number(b)),
        (1, 1) => a.as_str().cmp(&b.as_str()),
        (x, y) => x.cmp(&y),
    }
}

#[cfg(test)]
mod tests {
    use coracle_lib::filters::Filter;
    use coracle_lib::keys::{PublicKey, SecretKey};
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::events::EventHead;
    use crate::select::{Options, Order, Sample};

    /// The zap receipt invoice from NIP-57's example: 10 uBTC.
    const INVOICE: &str = "lnbc10u1p3unwfusp5t9r3yymhpfqculx78u027lxspgxcr2n2987mx2j55nnfs95nxnzqpp5jmrh92pfld78spqs78v9euf2385t83uvpwk9ldrlvf6ch7tpascqhp5zvkrmemgth3tufcvflmzjzfvjt023nazlhljz2n9hattj4f8jq8qxqyjw5qcqpjrzjqtc4fc44feggv7065fqe5m4ytjarg3repr5j9el35xhmtfexc42yczarjuqqfzqqqqqqqqlgqqqqqqgq9q9qxpqysgq079nkq507a5tw7xgttmj4u990j7wfggtrasah5gd4ywfr2pjcn29383tphp4t48gquelz9z78p4cq7ml3nrrphw5w6eckhjwmhezhnqpy6gyf0";

    fn event(
        n: u64,
        pubkey: PublicKey,
        content: &str,
        tags: &[&[&str]],
        created_at: i64,
    ) -> StoredEvent {
        StoredEvent {
            head: EventHead {
                id: Sha256::digest(n.to_be_bytes()).into(),
                pubkey,
                created_at,
                kind: 7,
                tag_values: Vec::new(),
            },
            content: content.to_string(),
            tags: serde_json::to_string(tags).unwrap(),
            sig: [0; 64],
        }
    }

    fn column(text: &str) -> Column {
        serde_json::from_str(text).unwrap()
    }

    fn run(events: &[StoredEvent], columns: &[&str], options: Value) -> Selection {
        let columns = columns.iter().map(|text| column(text)).collect();
        let options: Options = serde_json::from_value(options).unwrap();
        let query = Query::new(Filter::new(), columns, options).unwrap();
        Plan::new(query).unwrap().evaluate(events)
    }

    fn sorted(mut rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
        rows.sort_by_key(|row| row.iter().map(Value::to_string).collect::<Vec<_>>());
        rows
    }

    #[test]
    fn groups_events_and_computes_every_aggregate_per_group() {
        let (a, b) = (
            SecretKey::generate().public_key(),
            SecretKey::generate().public_key(),
        );
        let events = [
            event(1, a, "+", &[], 10),
            event(2, a, "+", &[], 30),
            event(3, b, "+", &[], 20),
            event(4, b, "🔥", &[], 5),
        ];
        let columns = [
            r#""content""#,
            r#"{"count": "id"}"#,
            r#"{"distinct": "pubkey"}"#,
            r#"{"sum": "created_at"}"#,
            r#"{"min": "created_at"}"#,
            r#"{"max": "created_at"}"#,
        ];

        assert_eq!(
            sorted(run(&events, &columns, json!({})).rows),
            vec![
                vec![
                    json!("+"),
                    json!(3),
                    json!(2),
                    json!(60),
                    json!(10),
                    json!(30)
                ],
                vec![
                    json!("🔥"),
                    json!(1),
                    json!(1),
                    json!(5),
                    json!(5),
                    json!(5)
                ],
            ]
        );
    }

    #[test]
    fn splits_array_values_into_a_row_per_distinct_element_and_combination() {
        let a = SecretKey::generate().public_key();
        let events = [
            event(1, a, "+", &[&["t", "a"], &["t", "b"], &["t", "a"]], 1),
            event(2, a, "-", &[&["t", "a"]], 1),
            event(3, a, "+", &[], 1),
        ];
        let columns = [
            r#""tags[?[0]=='t'][1]""#,
            r#""content""#,
            r#"{"count": "id"}"#,
        ];

        assert_eq!(
            sorted(run(&events, &columns, json!({})).rows),
            vec![
                vec![json!("a"), json!("+"), json!(1)],
                vec![json!("a"), json!("-"), json!(1)],
                vec![json!("b"), json!("+"), json!(1)],
            ]
        );
    }

    #[test]
    fn returns_one_row_without_grouping_columns_and_distinct_groups_without_aggregates() {
        let a = SecretKey::generate().public_key();
        let columns = [
            r#"{"count": "id"}"#,
            r#"{"sum": "created_at"}"#,
            r#"{"max": "created_at"}"#,
        ];

        assert_eq!(
            run(&[], &columns, json!({})).rows,
            vec![vec![json!(0), json!(0), Value::Null]]
        );
        assert_eq!(
            run(
                &[event(1, a, "+", &[], 1), event(2, a, "+", &[], 2)],
                &[r#""content""#],
                json!({})
            )
            .rows,
            vec![vec![json!("+")]]
        );
    }

    #[test]
    fn skips_events_failing_where_and_keeps_each_author_s_newest_with_latest() {
        let (a, b) = (
            SecretKey::generate().public_key(),
            SecretKey::generate().public_key(),
        );
        let events = [
            event(1, a, "yes", &[], 1),
            event(2, a, "no", &[], 2),
            event(3, b, "yes", &[&["skip", "1"]], 1),
        ];
        let options = json!({"where": "!(tags[?[0]=='skip'])", "latest": "pubkey"});

        assert_eq!(
            run(&events, &[r#""content""#, r#"{"count": "id"}"#], options).rows,
            vec![vec![json!("no"), json!(1)]]
        );
    }

    #[test]
    fn sorts_rows_with_nulls_last_keeps_the_limit_and_writes_large_sums_as_strings() {
        let a = SecretKey::generate().public_key();
        let big = 1i64 << 52;
        let events = [
            event(1, a, "a", &[], big),
            event(2, a, "a", &[], big),
            event(3, a, "b", &[], 3),
            event(4, a, "c", &[&["x", "1"]], 1000),
        ];
        let columns = [
            r#""content""#,
            r#"{"sum": "created_at"}"#,
            r#"{"max": "to_number(tags[0][1])"}"#,
        ];
        let by_max = run(&events, &columns, json!({"order": [2, "desc"], "limit": 2}));
        let by_sum = run(&events, &columns[..2], json!({"order": [1, "desc"]}));

        assert_eq!(by_max.rows.len(), 2);
        assert_eq!(by_max.rows[0], vec![json!("c"), json!(1000), json!(1)]);
        assert_eq!(
            by_sum.rows[0],
            vec![json!("a"), json!((big * 2).to_string())]
        );
    }

    #[test]
    fn gives_null_for_expressions_that_fail_at_runtime() {
        let a = SecretKey::generate().public_key();
        let columns = [r#"{"count": "length(created_at)"}"#, r#"{"count": "id"}"#];

        assert_eq!(
            run(&[event(1, a, "+", &[], 1)], &columns, json!({})).rows,
            vec![vec![json!(0), json!(1)]]
        );
    }

    #[test]
    fn refuses_expressions_that_do_not_parse_or_call_unknown_functions() {
        assert!(Expression::compile("tags[?").is_err());
        assert_eq!(
            Expression::compile("lower(content)").err().unwrap(),
            "unknown function lower"
        );
    }

    #[test]
    fn provides_from_json_bolt11_and_bucket() {
        let a = SecretKey::generate().public_key();
        let zap = to_variable(
            &event(
                1,
                a,
                "",
                &[&["bolt11", INVOICE], &["description", r#"{"pubkey":"d"}"#]],
                90061,
            )
            .to_json(false),
        )
        .unwrap();
        let read = |source: &str| Expression::compile(source).unwrap().evaluate(&zap);

        assert_eq!(
            read("bolt11(tags[?[0]=='bolt11'][1] | [0]).msats"),
            json!(1_000_000)
        );
        assert_eq!(read("bolt11(content)"), Value::Null);
        assert_eq!(
            read("from_json(tags[?[0]=='description'][1] | [0]).pubkey"),
            json!("d")
        );
        assert_eq!(read("from_json(content)"), Value::Null);
        assert_eq!(read("bucket(created_at, `86400`)"), json!(86400));
        assert_eq!(read("to_number(`\"21000\"`)"), json!(21000));
    }

    #[test]
    fn ranks_items_as_the_typescript_client_does() {
        let value = json!({"b": 21000.0, "a": "é", "c": [1.5, null]});

        assert_eq!(rank("salt", &[0xab; 32]), 0.864115113405674);
        assert_eq!(
            canonical(&normalize(value.clone())),
            r#"{"a":"é","b":21000,"c":[1.5,null]}"#
        );
        assert_eq!(
            rank("salt", canonical(&normalize(value)).as_bytes()),
            0.6439422860682208
        );
    }

    #[test]
    fn samples_the_lowest_ranked_items_whatever_order_events_arrive_in() {
        let authors: Vec<PublicKey> = (0..50)
            .map(|_| SecretKey::generate().public_key())
            .collect();
        let events: Vec<StoredEvent> = (0..300)
            .map(|i| {
                let content = if i % 2 == 0 { "-" } else { "+" };
                event(i, authors[i as usize % 50], content, &[], i as i64 + 1)
            })
            .collect();
        let reversed: Vec<StoredEvent> = events.iter().rev().cloned().collect();
        let columns = [
            r#""content""#,
            r#"{"count": "id"}"#,
            r#"{"sum": "created_at"}"#,
            r#"{"distinct": "pubkey"}"#,
        ];
        let options = json!({"salt": "pepper", "sample": 16});
        let forward = run(&events, &columns, options.clone());
        let backward = run(&reversed, &columns, options);
        let row = forward
            .rows
            .iter()
            .position(|row| row[0] == json!("+"))
            .unwrap();
        let samples = &forward.samples.as_ref().unwrap()[row];
        let mut lowest: Vec<(f64, String)> = events
            .iter()
            .filter(|event| event.content == "+")
            .map(|event| (rank("pepper", &event.head.id), event.head.id_hex()))
            .collect();
        lowest.sort_by(|a, b| a.0.total_cmp(&b.0));
        let lowest: Vec<Value> = lowest
            .into_iter()
            .take(16)
            .map(|(_, id)| json!(id))
            .collect();

        assert_eq!(forward.rows, backward.rows);
        assert_eq!(samples[0], None);
        assert_eq!(samples[1], Some(lowest));
        assert_eq!(samples[2], backward.samples.as_ref().unwrap()[row][2]);
        assert_eq!(samples[3].as_ref().map(Vec::len), Some(16));
    }

    #[test]
    fn checks_that_columns_and_options_agree() {
        let order = Options {
            order: Some(Order {
                column: 3,
                descending: true,
            }),
            ..Options::default()
        };

        assert!(Query::new(Filter::new(), vec![column(r#"{"count": "id"}"#)], order).is_err());
        assert!(Query::new(Filter::new(), Vec::new(), Options::default()).is_err());
        assert!(serde_json::from_value::<Options>(json!({"sample": 10})).is_err());
        assert!(serde_json::from_value::<Column>(json!({"median": "id"})).is_err());
        assert_eq!(
            serde_json::from_value::<Options>(json!({"salt": "s", "sample": 2}))
                .unwrap()
                .sample,
            Some(Sample {
                salt: "s".to_string(),
                size: 2
            })
        );
    }
}
