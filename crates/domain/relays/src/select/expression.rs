use std::sync::{Arc, LazyLock};

use jmespath::ast::Ast;
use jmespath::functions::{ArgumentType, CustomFunction, Signature};
use jmespath::{Context, Rcvar, Runtime, Variable, interpret};
use serde_json::{Number, Value, json};

use super::bolt11;

/// The JMESPath specification's functions, and those SELECT adds: `from_json`,
/// `bolt11` and `bucket`.
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    let mut runtime = Runtime::new();
    runtime.register_builtin_functions();
    runtime.register_function(
        "from_json",
        Box::new(CustomFunction::new(
            Signature::new(vec![ArgumentType::String], None),
            Box::new(|args, _| {
                let text = args[0].as_string().map(String::as_str).unwrap_or_default();
                Ok(Rcvar::new(
                    Variable::from_json(text).unwrap_or(Variable::Null),
                ))
            }),
        )),
    );
    runtime.register_function(
        "bolt11",
        Box::new(CustomFunction::new(
            Signature::new(vec![ArgumentType::String], None),
            Box::new(|args, _| {
                let invoice = args[0].as_string().and_then(|text| bolt11::decode(text));
                let value = invoice.map_or(Value::Null, |invoice| {
                    json!({
                        "msats": invoice.msats,
                        "payment_hash": invoice.payment_hash,
                        "description_hash": invoice.description_hash,
                        "created_at": invoice.created_at,
                    })
                });
                Ok(Rcvar::new(Variable::try_from(value)?))
            }),
        )),
    );
    runtime.register_function(
        "bucket",
        Box::new(CustomFunction::new(
            Signature::new(vec![ArgumentType::Number, ArgumentType::Number], None),
            Box::new(|args, _| {
                let (value, size) = (args[0].as_number(), args[1].as_number());
                let bucket = match (value, size) {
                    (Some(value), Some(size)) if size > 0.0 => (value / size).floor() * size,
                    _ => return Ok(Rcvar::new(Variable::Null)),
                };
                Ok(Rcvar::new(Variable::try_from(to_number(bucket))?))
            }),
        )),
    );
    runtime
});

/// The most bytes an expression may hold.
pub const MAX_EXPRESSION_BYTES: usize = 1024;

/// How deeply an expression's parentheses, brackets and braces may nest.
pub const MAX_EXPRESSION_DEPTH: usize = 8;

/// The most operators an expression may hold. Chains such as `a.b.c` or `!!a` don't
/// nest, but parsing and evaluating recurse through them as through brackets, and a
/// few hundred levels overflow a thread's stack, which aborts the process.
pub const MAX_EXPRESSION_OPERATORS: usize = 64;

/// A compiled JMESPath expression, evaluated against one event at a time.
pub struct Expression {
    source: String,
    ast: Ast,
}

impl Expression {
    /// Compile an expression, refusing one that is too large, that doesn't parse or
    /// that calls a function the relay doesn't have.
    pub fn compile(source: &str) -> Result<Self, String> {
        check_size(source)?;
        let ast = jmespath::parse(source)
            .map_err(|error| format!("invalid expression {source}: {error}"))?;
        check_functions(&ast)?;

        Ok(Self {
            source: source.to_string(),
            ast,
        })
    }

    /// The expression's value for an event, or null if evaluating it fails.
    pub fn evaluate(&self, event: &Rcvar) -> Value {
        let mut context = Context::new(&self.source, &RUNTIME);
        interpret(event, &self.ast, &mut context)
            .ok()
            .and_then(|result| serde_json::to_value(&*result).ok())
            .map_or(Value::Null, normalize)
    }
}

/// An event as expressions read it.
pub fn to_variable(json: &str) -> Option<Rcvar> {
    Variable::from_json(json).ok().map(Arc::new)
}

/// JMESPath's truthiness: false, null, and empty strings, arrays and objects are
/// false.
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(text) => !text.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        _ => true,
    }
}

/// A float that holds an integer the JSON way, as an integer, so that `21000.0`
/// and `21000` are the same value.
pub fn normalize(value: Value) -> Value {
    match value {
        Value::Number(number) => number.as_f64().map_or(Value::Number(number), to_number),
        Value::Array(values) => Value::Array(values.into_iter().map(normalize).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, normalize(value)))
                .collect(),
        ),
        other => other,
    }
}

/// The largest integer a double holds exactly, which JSON numbers stay within.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// A number as JSON, written as an integer when it holds one exactly.
fn to_number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER {
        Value::Number(Number::from(value as i64))
    } else {
        Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// Refuse an expression longer, more deeply nested or with more operators than the
/// limits allow, reading it before the recursive parser does. What literals and
/// quoted names hold doesn't count.
fn check_size(source: &str) -> Result<(), String> {
    if source.len() > MAX_EXPRESSION_BYTES {
        return Err(format!(
            "an expression may hold at most {MAX_EXPRESSION_BYTES} bytes"
        ));
    }

    let (mut depth, mut operators) = (0usize, 0usize);
    let (mut quote, mut escaped) = (None, false);
    for c in source.chars() {
        if let Some(closing) = quote {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                _ if c == closing => quote = None,
                _ => {}
            }
            continue;
        }

        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '(' | '[' | '{' => {
                depth += 1;
                operators += 1;
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '.' | '|' | '&' | '!' | '*' | '<' | '>' | '=' => operators += 1,
            _ => {}
        }

        if depth > MAX_EXPRESSION_DEPTH {
            return Err(format!(
                "an expression may nest at most {MAX_EXPRESSION_DEPTH} levels deep"
            ));
        }
        if operators > MAX_EXPRESSION_OPERATORS {
            return Err(format!(
                "an expression may hold at most {MAX_EXPRESSION_OPERATORS} operators"
            ));
        }
    }

    Ok(())
}

/// Refuse an expression that calls a function the runtime doesn't have, which
/// JMESPath would otherwise only notice when evaluating it.
fn check_functions(ast: &Ast) -> Result<(), String> {
    match ast {
        Ast::Function { name, args, .. } => {
            if RUNTIME.get_function(name).is_none() {
                return Err(format!("unknown function {name}"));
            }
            args.iter().try_for_each(check_functions)
        }
        Ast::Comparison { lhs, rhs, .. }
        | Ast::Projection { lhs, rhs, .. }
        | Ast::And { lhs, rhs, .. }
        | Ast::Or { lhs, rhs, .. }
        | Ast::Subexpr { lhs, rhs, .. } => {
            check_functions(lhs)?;
            check_functions(rhs)
        }
        Ast::Condition {
            predicate, then, ..
        } => {
            check_functions(predicate)?;
            check_functions(then)
        }
        Ast::Expref { ast, .. } => check_functions(ast),
        Ast::Flatten { node, .. } | Ast::Not { node, .. } | Ast::ObjectValues { node, .. } => {
            check_functions(node)
        }
        Ast::MultiList { elements, .. } => elements.iter().try_for_each(check_functions),
        Ast::MultiHash { elements, .. } => elements
            .iter()
            .try_for_each(|pair| check_functions(&pair.value)),
        Ast::Identity { .. }
        | Ast::Field { .. }
        | Ast::Index { .. }
        | Ast::Literal { .. }
        | Ast::Slice { .. } => Ok(()),
    }
}
