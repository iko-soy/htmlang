//! Values: what a `$name` holds, and the rules every part of htmlang
//! applies to them (truthiness, comparison, how a value prints).
//!
//! A value is text, a number, `true` or `false`, a list or a record. A
//! `@let`, a parameter, `@each` and `@data` bind a whole value to a name,
//! so a list stays a list and a record a record wherever it is passed.
//!
//! - **Text** is what the source (or a JSON string) says. Written without
//!   quotes, it reads as a number when it is one (`3`, `0.5`) and as false
//!   when it is `false`, since that is how htmlang writes numbers and flags
//!   (`@let gap 8`, `@card [dot false]`). **Quoted text** (`"0"`) is always
//!   text, and a CSS value writes it with its quotes.
//! - **Numbers** come from arithmetic, ranges, `length()` and an `@each`
//!   index, and print without float noise: at most four decimals.
//! - **Lists** come from commas in the source (`apple, banana`), from a
//!   range (`1..5`), from JSON arrays and from list functions. A list
//!   written in the source prints as it was written; any other prints its
//!   items joined with `, `.
//! - **Records** come from JSON objects (and `@let name.field value`); a
//!   record has no text of its own, and a field it doesn't have is empty.
//!
//! False: `false`, empty text, `0` (as a number or as unquoted text), an
//! empty list and an empty record. Everything else is true.
//!
//! `==` and `!=` compare numbers as numbers (`1.0 == 1`), lists and records
//! item by item, and anything else as text, exactly (`#FFF != #fff`). `<`,
//! `>`, `<=` and `>=` compare numbers as numbers and text by character
//! order; a list or a record can't be ordered.

use std::fmt;
use std::rc::Rc;

use crate::ast::Quoted;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Text, as written or loaded.
    Str(String),
    /// Quoted text: what it says, and how a CSS value writes it.
    Quoted(Quoted),
    Num(f64),
    Bool(bool),
    List(List),
    /// A record's fields, in order.
    Record(Rc<Vec<(String, Value)>>),
}

/// A list's items, and how the source wrote it, which is how it prints.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub items: Rc<Vec<Value>>,
    pub written: Option<Rc<str>>,
}

/// The most items a range may have, so a typo can't exhaust memory.
pub const MAX_RANGE: usize = 100_000;

impl Value {
    /// Empty text: what a field a record doesn't have holds.
    pub fn empty() -> Value {
        Value::Str(String::new())
    }

    /// A list of `items`, which prints them joined with `, `.
    pub fn list(items: Vec<Value>) -> Value {
        Value::List(List {
            items: Rc::new(items),
            written: None,
        })
    }

    /// See the module documentation.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !(s.is_empty() || s == "false" || parse_number(s) == Some(0.0)),
            Value::Quoted(q) => !q.text.is_empty(),
            Value::List(list) => !list.items.is_empty(),
            Value::Record(fields) => !fields.is_empty(),
        }
    }

    /// The number a value is: a number, or unquoted text that is one.
    pub fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Str(s) => parse_number(s),
            _ => None,
        }
    }

    /// Whether `.field` reads into it: a record or a list.
    pub fn has_fields(&self) -> bool {
        matches!(self, Value::Record(_) | Value::List(_))
    }

    /// The field `name` of a record, or the item at index `name` of a list:
    /// empty when there is none. `None` for a value without fields.
    pub fn field(&self, name: &str) -> Option<Value> {
        match self {
            Value::Record(fields) => Some(
                fields
                    .iter()
                    .find(|(key, _)| key == name)
                    .map_or_else(Value::empty, |(_, value)| value.clone()),
            ),
            Value::List(list) => Some(
                name.parse::<usize>()
                    .ok()
                    .and_then(|i| list.items.get(i).cloned())
                    .unwrap_or_else(Value::empty),
            ),
            _ => None,
        }
    }

    /// How CSS writes it, when it is quoted text.
    pub fn quoted_css(&self) -> Option<&str> {
        match self {
            Value::Quoted(q) => Some(&q.css),
            _ => None,
        }
    }

    /// What kind of value it is, for messages: `a list of 3 items`.
    pub fn describe(&self) -> String {
        match self {
            Value::Str(_) | Value::Quoted(_) => "text".to_string(),
            Value::Num(_) => "a number".to_string(),
            Value::Bool(_) => "true or false".to_string(),
            Value::List(list) => match list.items.len() {
                1 => "a list of 1 item".to_string(),
                n => format!("a list of {} items", n),
            },
            Value::Record(_) => "a record".to_string(),
        }
    }

    /// `==`: see the module documentation.
    pub fn equals(&self, other: &Value) -> bool {
        if let (Some(a), Some(b)) = (self.as_num(), other.as_num()) {
            return a == b;
        }
        match (self, other) {
            (Value::List(a), Value::List(b)) => {
                a.items.len() == b.items.len()
                    && a.items.iter().zip(b.items.iter()).all(|(x, y)| x.equals(y))
            }
            (Value::Record(a), Value::Record(b)) => {
                a.len() == b.len()
                    && a.iter().all(|(key, x)| {
                        b.iter()
                            .find(|(k, _)| k == key)
                            .is_some_and(|(_, y)| x.equals(y))
                    })
            }
            (Value::List(_) | Value::Record(_), _) | (_, Value::List(_) | Value::Record(_)) => {
                false
            }
            _ => self.to_string() == other.to_string(),
        }
    }

    /// A comparison operator (`==`, `!=`, `<`, `>`, `<=`, `>=`).
    pub fn compare(&self, op: &str, other: &Value) -> Result<bool, String> {
        match op {
            "==" => return Ok(self.equals(other)),
            "!=" => return Ok(!self.equals(other)),
            _ => {}
        }
        let order = match (self.as_num(), other.as_num()) {
            (Some(a), Some(b)) => a.partial_cmp(&b),
            _ => {
                for side in [self, other] {
                    if matches!(side, Value::List(_) | Value::Record(_)) {
                        return Err(format!(
                            "`{}` compares numbers or text, not {}",
                            op,
                            side.describe()
                        ));
                    }
                }
                Some(self.to_string().cmp(&other.to_string()))
            }
        };
        let Some(order) = order else {
            return Ok(false);
        };
        Ok(match op {
            "<" => order.is_lt(),
            ">" => order.is_gt(),
            "<=" => order.is_le(),
            ">=" => order.is_ge(),
            _ => return Err(format!("unknown comparison `{}`", op)),
        })
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => f.write_str(s),
            Value::Quoted(q) => f.write_str(&q.text),
            Value::Num(n) => f.write_str(&format_number(*n)),
            Value::Bool(b) => write!(f, "{}", b),
            Value::List(list) => match &list.written {
                Some(written) => f.write_str(written),
                None => {
                    // A list of records (or of lists) has no text of its own
                    let items: Vec<String> = list
                        .items
                        .iter()
                        .filter(|item| !item.has_fields())
                        .map(Value::to_string)
                        .filter(|s| !s.is_empty())
                        .collect();
                    f.write_str(&items.join(", "))
                }
            },
            // A record has no text of its own
            Value::Record(_) => Ok(()),
        }
    }
}

/// A number as htmlang prints it: a whole number without decimals, any
/// other rounded to at most four decimals, so `100 / 3` is `33.3333`.
pub fn format_number(n: f64) -> String {
    if !n.is_finite() {
        return "0".to_string();
    }
    let rounded = (n * 10_000.0).round() / 10_000.0;
    if rounded.fract() == 0.0 && rounded.abs() < 1e15 {
        // `-0` is `0`
        return format!("{}", rounded as i64);
    }
    format!("{}", rounded)
}

/// The number text is, when it is written as one: digits with an optional
/// sign, point and exponent (`3`, `-0.5`, `.5`, `1e3`), and nothing else,
/// so `inf` or `10px` aren't numbers.
pub fn parse_number(text: &str) -> Option<f64> {
    let s = text.trim();
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    if !digits.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        || !digits
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
    {
        return None;
    }
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// The whole numbers from `start` to `end`, both included, `step` apart:
/// counting down when `start` is greater than `end`.
pub fn range(start: f64, end: f64, step: f64) -> Result<Value, String> {
    // Exact as whole numbers up to 2^53
    let whole = |n: f64| n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_992.0;
    if !whole(start) || !whole(end) {
        return Err(format!(
            "a range goes from one whole number to another, not `{}..{}`",
            format_number(start),
            format_number(end)
        ));
    }
    if !whole(step) || step < 1.0 {
        return Err(format!(
            "a range's step is a whole number above 0, not `{}`",
            format_number(step)
        ));
    }
    whole_range(start as i64, end as i64, step as i64)
}

/// [`range`] of whole numbers. A number too large for a number value to
/// hold exactly is text, which still reads as that number.
pub fn whole_range(start: i64, end: i64, step: i64) -> Result<Value, String> {
    if step < 1 {
        return Err(format!(
            "a range's step is a whole number above 0, not `{}`",
            step
        ));
    }
    let count = (start.abs_diff(end) / step.unsigned_abs()) as usize + 1;
    if count > MAX_RANGE {
        return Err(format!(
            "the range `{}..{}` has {} items, more than the {} a range may have",
            start, end, count, MAX_RANGE
        ));
    }
    let exact = |n: i64| n.unsigned_abs() <= 1 << 53;
    let mut items = Vec::with_capacity(count);
    let mut n = start;
    for i in 0..count {
        items.push(match exact(n) {
            true => Value::Num(n as f64),
            false => Value::Str(n.to_string()),
        });
        if i + 1 < count {
            n = match start > end {
                true => n - step,
                false => n + step,
            };
        }
    }
    Ok(Value::list(items))
}

/// A range written as a whole value, `A..B` or `A..B step N` with whole
/// numbers (after its variables are filled in): `None` when `text` isn't
/// one.
pub fn written_range(text: &str) -> Option<Result<Value, String>> {
    let (start, rest) = text.trim().split_once("..")?;
    let (end, step) = match rest.split_once(" step ") {
        Some((end, step)) => (end, Some(step)),
        None => (rest, None),
    };
    let whole = |s: &str| s.trim().parse::<i64>().ok();
    let (start, end) = (whole(start)?, whole(end)?);
    let step = match step {
        Some(step) => whole(step)?,
        None => 1,
    };
    Some(whole_range(start, end, step).map(|value| match value {
        Value::List(list) => Value::List(List {
            written: Some(text.trim().into()),
            ..list
        }),
        other => other,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    #[test]
    fn truthiness_is_defined_once() {
        for falsy in [
            text(""),
            text("false"),
            text("0"),
            text("0.0"),
            Value::Num(0.0),
            Value::Bool(false),
            Value::list(vec![]),
        ] {
            assert!(!falsy.truthy(), "{:?}", falsy);
        }
        let quoted_zero = Value::Quoted(Quoted {
            text: "0".into(),
            css: "\"0\"".into(),
        });
        for truthy in [text("no"), text("False"), text("0px"), quoted_zero] {
            assert!(truthy.truthy(), "{:?}", truthy);
        }
    }

    #[test]
    fn comparison() {
        assert!(text("1.0").equals(&Value::Num(1.0)));
        assert!(!text("#FFF").equals(&text("#fff")));
        assert!(Value::Bool(true).equals(&text("true")));
        assert_eq!(text("2").compare("<", &text("10")), Ok(true));
        assert_eq!(text("b").compare(">", &text("a")), Ok(true));
        assert!(Value::list(vec![]).compare("<", &text("a")).is_err());
        // `inf` and `10px` are text
        assert_eq!(parse_number("inf"), None);
        assert_eq!(parse_number("10px"), None);
        assert_eq!(parse_number(" -.5 "), Some(-0.5));
    }

    #[test]
    fn numbers_print_without_float_noise() {
        assert_eq!(format_number(100.0 / 3.0), "33.3333");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(3.5), "3.5");
        assert_eq!(format_number(16.0), "16");
    }

    #[test]
    fn ranges() {
        let items = |v: Value| match v {
            Value::List(list) => list.items.iter().map(Value::to_string).collect::<Vec<_>>(),
            other => panic!("{:?}", other),
        };
        assert_eq!(items(range(1.0, 3.0, 1.0).unwrap()), ["1", "2", "3"]);
        assert_eq!(items(range(10.0, 0.0, 5.0).unwrap()), ["10", "5", "0"]);
        assert!(range(1.0, 5.0, 0.0).is_err());
        assert!(range(0.5, 5.0, 1.0).is_err());
        assert!(range(0.0, 1e9, 1.0).is_err());
        let written = written_range(" 1..5 step 2").unwrap().unwrap();
        assert_eq!(written.to_string(), "1..5 step 2");
        assert_eq!(items(written), ["1", "3", "5"]);
        assert!(written_range("a..b").is_none());
        assert!(written_range("1..2..3").is_none());
    }
}
