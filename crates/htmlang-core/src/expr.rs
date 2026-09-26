//! Expressions: conditions (`@if $count > 2 and not $hidden`), computed
//! values (`@let gap = $base * 2`) and `${...}` in text and values
//! (`${if($featured, 24, 0)}`).
//!
//! Expressions are tokenized and evaluated as a whole. Variables are looked
//! up while evaluating, never pasted into the text first, so a value that
//! happens to contain `==` or spaces can't change how an expression parses.
//!
//! Only what decides the result is evaluated: `if()` evaluates the branch
//! it takes, `and` and `or` stop at the first side that decides. The rest
//! is still parsed (and its function names and argument counts checked),
//! so `$n != 0 and 10 / $n > 1` is false for `$n` = 0, not an error.
//!
//! ```text
//! expr    := or
//! or      := and ("or" and)*
//! and     := not ("and" not)*
//! not     := "not" not | compare
//! compare := sum (("==" | "!=" | "<" | ">" | "<=" | ">=") sum)?
//! sum     := product (("+" | "-") product)*
//! product := unary (("*" | "/" | "%") unary)*
//! unary   := "-" unary | primary
//! primary := NUMBER | "STRING" | $VAR | WORD | true | false
//!          | NAME "(" expr ("," expr)* ")" | "(" expr ")"
//! ```
//!
//! A bare word (`dark`, `red`, `#3b82f6`, `10px`) is a string. A variable
//! loaded from a JSON array is a list: `length` counts its items and
//! `contains` tests membership. Functions:
//! `if(cond, a, b)` (`b` may be left out: empty), tests (`contains(s, x)`, `starts-with(s, x)`,
//! `ends-with(s, x)`), text (`uppercase`, `lowercase`, `capitalize`, `trim`,
//! `length`, `reverse`, `truncate(s, n)`, `replace(s, old, new)`,
//! `default(s, fallback)`) and colors (`lighten(c, pct)`, `darken(c, pct)`,
//! `alpha(c, a)`, `mix(c1, c2, pct)`). In text, `${expr}` interpolates an
//! expression.
//!
//! A `$name` ends as it does in text (see `interp.rs`), so `$post.title`
//! reads a field and `$a..$b` is two names. An undefined name is an error.

use std::fmt;

use crate::interp::{self, Problem, Reference, Scope};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f64),
    Str(String),
    Bool(bool),
    /// A list's items as text (from a JSON array).
    List(Vec<String>),
}

impl Value {
    /// Empty strings, `false`, `0` and `"false"` / `"0"` are false.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty() && s != "false" && s != "0",
            Value::List(items) => !items.is_empty(),
        }
    }

    fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Str(s) => s.trim().parse().ok(),
            Value::Bool(_) | Value::List(_) => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => write!(f, "{}", *n as i64),
            Value::Num(n) => write!(f, "{}", n),
            Value::Str(s) => f.write_str(s),
            Value::Bool(b) => write!(f, "{}", b),
            Value::List(items) => f.write_str(&items.join(", ")),
        }
    }
}

/// Why an expression has no value.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// `$name` has no definition; `offset` is where the `$` is.
    Undefined { name: String, offset: usize },
    /// Anything else: a syntax error, a type error, division by zero.
    Invalid(String),
}

impl Error {
    /// This error as a problem of a text in which the expression starts
    /// at `base`.
    pub(crate) fn at(self, base: usize) -> Problem {
        match self {
            Error::Undefined { name, offset } => Problem::Undefined {
                name,
                offset: base + offset,
            },
            Error::Invalid(message) => Problem::Invalid {
                message,
                offset: base,
            },
        }
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Error::Invalid(message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Undefined { name, .. } => write!(f, "undefined variable '${}'", name),
            Error::Invalid(message) => f.write_str(message),
        }
    }
}

/// Evaluate `src` with the variables of `scope`. An undefined variable is
/// an error; a field that a record doesn't have is empty.
pub fn eval(src: &str, scope: &dyn Scope) -> Result<Value, Error> {
    let mut tokens = Vec::new();
    tokenize(src, scope, 0, &mut tokens)?;
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        scope,
        skip: false,
    };
    let value = parser.expr()?;
    match parser.tokens.get(parser.pos) {
        None => Ok(value),
        Some((tok, _)) => Err(Error::Invalid(format!(
            "unexpected {} in `{}`",
            tok,
            src.trim()
        ))),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Str(String),
    Var(String),
    Word(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Num(n) => write!(f, "`{}`", n),
            Token::Str(s) => write!(f, "`\"{}\"`", s),
            Token::Var(v) => write!(f, "`${}`", v),
            Token::Word(w) => write!(f, "`{}`", w),
            Token::Op(op) => write!(f, "`{}`", op),
            Token::LParen => f.write_str("`(`"),
            Token::RParen => f.write_str("`)`"),
            Token::Comma => f.write_str("`,`"),
        }
    }
}

const OPERATORS: &[&str] = &["==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%"];

/// Read `src` into tokens, each with its byte offset (plus `base`).
/// Variable names end as in text (see `interp.rs`), and `${...}` is a
/// variable (`${name}`) or a parenthesized expression.
fn tokenize(
    src: &str,
    scope: &dyn Scope,
    base: usize,
    tokens: &mut Vec<(Token, usize)>,
) -> Result<(), Error> {
    let is_word_char = |c: char| c.is_alphanumeric() || matches!(c, '#' | '.' | '_' | '-' | '%');
    let mut i = 0;
    while let Some(c) = src[i..].chars().next() {
        let at = base + i;
        if c.is_whitespace() {
            i += c.len_utf8();
        } else if c == '(' {
            tokens.push((Token::LParen, at));
            i += 1;
        } else if c == ')' {
            tokens.push((Token::RParen, at));
            i += 1;
        } else if c == ',' {
            tokens.push((Token::Comma, at));
            i += 1;
        } else if c == '"' {
            let start = i + 1;
            let end = src[start..]
                .find('"')
                .map(|p| start + p)
                .ok_or_else(|| format!("unclosed string in `{}`", src.trim()))?;
            tokens.push((Token::Str(src[start..end].to_string()), at));
            i = end + 1;
        } else if c == '$' {
            match interp::reference(&src[i + 1..], scope) {
                Some((Reference::Var(path), len)) => {
                    tokens.push((Token::Var(path), at));
                    i += 1 + len;
                }
                Some((Reference::Expr(inner), len)) => {
                    tokens.push((Token::LParen, at));
                    tokenize(inner, scope, at + 2, tokens)?;
                    tokens.push((Token::RParen, at + len));
                    i += 1 + len;
                }
                None => {
                    return Err(Error::Invalid(format!(
                        "`$` without a variable name in `{}`",
                        src.trim()
                    )));
                }
            }
        } else if let Some(op) = OPERATORS
            .iter()
            .find(|op| src[i..].starts_with(**op))
            // A `-` followed by a letter starts a word (`-webkit-box`);
            // inside words (`ease-in`) it is consumed by the word itself.
            .filter(|op| **op != "-" || !src[i + 1..].starts_with(char::is_alphabetic))
        {
            tokens.push((Token::Op(op), at));
            i += op.len();
        } else if is_word_char(c) {
            let start = i;
            while let Some(c) = src[i..].chars().next().filter(|&c| is_word_char(c)) {
                i += c.len_utf8();
            }
            let word = &src[start..i];
            tokens.push((
                match word.parse::<f64>() {
                    Ok(n) if !word.ends_with('.') => Token::Num(n),
                    _ => Token::Word(word.to_string()),
                },
                at,
            ));
        } else {
            return Err(Error::Invalid(format!(
                "unexpected `{}` in `{}`",
                c,
                src.trim()
            )));
        }
    }
    Ok(())
}

struct Parser<'a> {
    tokens: &'a [(Token, usize)],
    pos: usize,
    scope: &'a dyn Scope,
    /// Parsing a part whose value isn't needed (the branch `if()` doesn't
    /// take, the side of `and`/`or` that doesn't decide): it is read and
    /// checked for syntax, not evaluated.
    skip: bool,
}

/// The value a skipped part stands for; nothing reads it.
const SKIPPED: Value = Value::Bool(false);

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(token, _)| token)
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Some(Token::Word(w)) if w == word) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_op(&mut self, ops: &[&'static str]) -> Option<&'static str> {
        match self.peek() {
            Some(Token::Op(op)) if ops.contains(op) => {
                let op = *op;
                self.pos += 1;
                Some(op)
            }
            _ => None,
        }
    }

    fn expect(&mut self, token: Token) -> Result<(), Error> {
        match self.peek() {
            Some(t) if *t == token => {
                self.pos += 1;
                Ok(())
            }
            Some(t) => Err(format!("expected {} but found {}", token, t).into()),
            None => Err(format!("expected {} at the end", token).into()),
        }
    }

    /// Parse with `f`, skipping evaluation when `skip` (or when already
    /// skipping).
    fn skipping(
        &mut self,
        skip: bool,
        f: impl FnOnce(&mut Self) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        let outer = self.skip;
        self.skip |= skip;
        let value = f(self);
        self.skip = outer;
        value
    }

    fn expr(&mut self) -> Result<Value, Error> {
        let mut left = self.and()?;
        while self.eat_word("or") {
            let decided = left.truthy();
            let right = self.skipping(decided, Self::and)?;
            left = Value::Bool(decided || right.truthy());
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Value, Error> {
        let mut left = self.not()?;
        while self.eat_word("and") {
            let decided = !left.truthy();
            let right = self.skipping(decided, Self::not)?;
            left = Value::Bool(!decided && right.truthy());
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Value, Error> {
        if self.eat_word("not") {
            return Ok(Value::Bool(!self.not()?.truthy()));
        }
        self.compare()
    }

    fn compare(&mut self) -> Result<Value, Error> {
        let left = self.sum()?;
        let Some(op) = self.eat_op(&["==", "!=", "<=", ">=", "<", ">"]) else {
            return Ok(left);
        };
        let right = self.sum()?;
        if self.skip {
            return Ok(SKIPPED);
        }
        let numbers = left.as_num().zip(right.as_num());
        let (l, r) = (left.to_string(), right.to_string());
        Ok(Value::Bool(match (op, numbers) {
            ("==", Some((a, b))) => a == b,
            ("!=", Some((a, b))) => a != b,
            ("<", Some((a, b))) => a < b,
            (">", Some((a, b))) => a > b,
            ("<=", Some((a, b))) => a <= b,
            (">=", Some((a, b))) => a >= b,
            ("==", None) => l == r,
            ("!=", None) => l != r,
            ("<", None) => l < r,
            (">", None) => l > r,
            ("<=", None) => l <= r,
            (">=", None) => l >= r,
            _ => unreachable!(),
        }))
    }

    fn sum(&mut self) -> Result<Value, Error> {
        let mut left = self.product()?;
        while let Some(op) = self.eat_op(&["+", "-"]) {
            let right = self.product()?;
            left = self.arithmetic(op, &left, &right)?;
        }
        Ok(left)
    }

    fn product(&mut self) -> Result<Value, Error> {
        let mut left = self.unary()?;
        while let Some(op) = self.eat_op(&["*", "/", "%"]) {
            let right = self.unary()?;
            left = self.arithmetic(op, &left, &right)?;
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Value, Error> {
        if self.eat_op(&["-"]).is_some() {
            let value = self.unary()?;
            if self.skip {
                return Ok(SKIPPED);
            }
            return value
                .as_num()
                .map(|n| Value::Num(-n))
                .ok_or_else(|| Error::Invalid(format!("cannot negate `{}`", value)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Value, Error> {
        let (token, at) = self
            .tokens
            .get(self.pos)
            .cloned()
            .ok_or_else(|| "expected a value at the end".to_string())?;
        self.pos += 1;
        match token {
            Token::Num(n) => Ok(Value::Num(n)),
            Token::Str(_) | Token::Var(_) if self.skip => Ok(SKIPPED),
            Token::Str(s) => {
                let (text, problems) = interp::interpolate(&s, self.scope);
                match problems.into_iter().next() {
                    None => Ok(Value::Str(text)),
                    Some(Problem::Undefined { name, offset }) => Err(Error::Undefined {
                        name,
                        offset: at + 1 + offset,
                    }),
                    Some(Problem::Invalid { message, .. }) => Err(Error::Invalid(message)),
                }
            }
            Token::Var(name) => {
                interp::resolve(&name, self.scope).ok_or(Error::Undefined { name, offset: at })
            }
            Token::Word(w) if w == "true" => Ok(Value::Bool(true)),
            Token::Word(w) if w == "false" => Ok(Value::Bool(false)),
            Token::Word(name) if name == "if" && self.peek() == Some(&Token::LParen) => {
                self.pos += 1;
                self.choice()
            }
            Token::Word(name) if self.peek() == Some(&Token::LParen) => {
                self.pos += 1;
                let mut args = Vec::new();
                if self.peek() != Some(&Token::RParen) {
                    args.push(self.expr()?);
                    while self.peek() == Some(&Token::Comma) {
                        self.pos += 1;
                        args.push(self.expr()?);
                    }
                }
                self.expect(Token::RParen)?;
                check_call(&name, args.len())?;
                if self.skip {
                    return Ok(SKIPPED);
                }
                Ok(call(&name, args)?)
            }
            Token::Word(w) => Ok(Value::Str(w)),
            Token::LParen => {
                let value = self.expr()?;
                self.expect(Token::RParen)?;
                Ok(value)
            }
            other => Err(format!("unexpected {}", other).into()),
        }
    }
}

impl Parser<'_> {
    /// `if(cond, a, b)` after its `(`: the branch that `cond` picks is
    /// evaluated, the other only read. Without `b`, a false `cond` gives
    /// an empty value.
    fn choice(&mut self) -> Result<Value, Error> {
        let arguments =
            || Error::Invalid("if() takes 2 or 3 arguments: if(CONDITION, A, B)".into());
        let condition = self.expr()?;
        match self.peek() {
            Some(Token::RParen) | None => return Err(arguments()),
            _ => self.expect(Token::Comma)?,
        }
        let taken = condition.truthy();
        let then = self.skipping(!taken, Self::expr)?;
        let otherwise = if self.peek() == Some(&Token::Comma) {
            self.pos += 1;
            self.skipping(taken, Self::expr)?
        } else {
            Value::Str(String::new())
        };
        if self.peek() == Some(&Token::Comma) {
            return Err(arguments());
        }
        self.expect(Token::RParen)?;
        Ok(if taken { then } else { otherwise })
    }

    fn arithmetic(&self, op: &str, left: &Value, right: &Value) -> Result<Value, Error> {
        if self.skip {
            return Ok(SKIPPED);
        }
        Ok(arithmetic(op, left, right)?)
    }
}

fn arithmetic(op: &str, left: &Value, right: &Value) -> Result<Value, String> {
    let (Some(a), Some(b)) = (left.as_num(), right.as_num()) else {
        return Err(format!("`{} {} {}` needs numbers", left, op, right));
    };
    Ok(Value::Num(match op {
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        "/" if b == 0.0 => return Err("division by zero".to_string()),
        "/" => a / b,
        "%" if b == 0.0 => return Err("division by zero".to_string()),
        "%" => a % b,
        _ => unreachable!(),
    }))
}

/// The built-in functions (other than `if`) and how many arguments each
/// takes.
const FUNCTIONS: &[(&str, usize)] = &[
    ("contains", 2),
    ("starts-with", 2),
    ("ends-with", 2),
    ("uppercase", 1),
    ("lowercase", 1),
    ("capitalize", 1),
    ("trim", 1),
    ("length", 1),
    ("reverse", 1),
    ("truncate", 2),
    ("replace", 3),
    ("default", 2),
    ("lighten", 2),
    ("darken", 2),
    ("alpha", 2),
    ("mix", 3),
];

/// Check that `name` is a built-in function and gets `count` arguments.
fn check_call(name: &str, count: usize) -> Result<(), String> {
    let Some(&(_, arity)) = FUNCTIONS.iter().find(|(n, _)| *n == name) else {
        return Err(format!("unknown function `{}()`", name));
    };
    if count == arity {
        Ok(())
    } else {
        Err(format!(
            "{}() takes {} argument{}",
            name,
            arity,
            if arity == 1 { "" } else { "s" }
        ))
    }
}

/// Call a built-in function with the right number of arguments (see
/// [`check_call`]).
fn call(name: &str, args: Vec<Value>) -> Result<Value, String> {
    let text = |i: usize| args.get(i).map(|v| v.to_string()).unwrap_or_default();
    let num = |i: usize| {
        args.get(i)
            .and_then(Value::as_num)
            .ok_or_else(|| format!("{}() needs a number as argument {}", name, i + 1))
    };
    let color = |rgb: (u8, u8, u8)| Value::Str(format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2));
    Ok(match name {
        "contains" => Value::Bool(match &args[0] {
            Value::List(items) => items.contains(&text(1)),
            other => other.to_string().contains(&text(1)),
        }),
        "starts-with" => Value::Bool(text(0).starts_with(&text(1))),
        "ends-with" => Value::Bool(text(0).ends_with(&text(1))),
        "uppercase" => Value::Str(text(0).to_uppercase()),
        "lowercase" => Value::Str(text(0).to_lowercase()),
        "capitalize" => {
            let s = text(0);
            let mut chars = s.chars();
            Value::Str(match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => String::new(),
            })
        }
        "trim" => Value::Str(text(0).trim().to_string()),
        "length" => Value::Num(match &args[0] {
            Value::List(items) => items.len(),
            other => other.to_string().chars().count(),
        } as f64),
        "reverse" => Value::Str(text(0).chars().rev().collect()),
        "truncate" => {
            let (s, n) = (text(0), num(1)? as usize);
            Value::Str(if s.chars().count() > n {
                format!("{}...", s.chars().take(n).collect::<String>())
            } else {
                s
            })
        }
        "replace" => Value::Str(text(0).replace(&text(1), &text(2))),
        "default" => {
            let s = text(0);
            Value::Str(if s.is_empty() { text(1) } else { s })
        }
        "lighten" | "darken" | "alpha" | "mix" => {
            let Some(rgb) = parse_hex_rgb(&text(0)) else {
                return Err(format!("{}() needs a hex color, got `{}`", name, text(0)));
            };
            match name {
                "lighten" => color(lighten_color(rgb, num(1)? / 100.0)),
                "darken" => color(darken_color(rgb, num(1)? / 100.0)),
                "alpha" => {
                    let a = (num(1)?.clamp(0.0, 1.0) * 255.0) as u8;
                    Value::Str(format!("#{:02x}{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2, a))
                }
                _ => {
                    let Some(other) = parse_hex_rgb(&text(1)) else {
                        return Err(format!("mix() needs a hex color, got `{}`", text(1)));
                    };
                    color(mix_colors(rgb, other, num(2)? / 100.0))
                }
            }
        }
        _ => return Err(format!("unknown function `{}()`", name)),
    })
}

pub(crate) fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#')?;
    if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()?;
            let g = u8::from_str_radix(&s[1..2], 16).ok()?;
            let b = u8::from_str_radix(&s[2..3], 16).ok()?;
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn lighten_color(rgb: (u8, u8, u8), amount: f64) -> (u8, u8, u8) {
    let r = rgb.0 as f64 + (255.0 - rgb.0 as f64) * amount.clamp(0.0, 1.0);
    let g = rgb.1 as f64 + (255.0 - rgb.1 as f64) * amount.clamp(0.0, 1.0);
    let b = rgb.2 as f64 + (255.0 - rgb.2 as f64) * amount.clamp(0.0, 1.0);
    (r.round() as u8, g.round() as u8, b.round() as u8)
}

fn darken_color(rgb: (u8, u8, u8), amount: f64) -> (u8, u8, u8) {
    let factor = 1.0 - amount.clamp(0.0, 1.0);
    let r = (rgb.0 as f64 * factor).round() as u8;
    let g = (rgb.1 as f64 * factor).round() as u8;
    let b = (rgb.2 as f64 * factor).round() as u8;
    (r, g, b)
}

fn mix_colors(c1: (u8, u8, u8), c2: (u8, u8, u8), weight: f64) -> (u8, u8, u8) {
    let w = weight.clamp(0.0, 1.0);
    let r = (c1.0 as f64 * (1.0 - w) + c2.0 as f64 * w).round() as u8;
    let g = (c1.1 as f64 * (1.0 - w) + c2.1 as f64 * w).round() as u8;
    let b = (c1.2 as f64 * (1.0 - w) + c2.2 as f64 * w).round() as u8;
    (r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::interp::tests::Map;

    fn scope() -> Map {
        Map::new(&[
            ("tags#", "2"),
            ("tags.0", "rust"),
            ("tags.1", "web dev"),
            ("count", "3"),
            ("name", "World"),
            ("theme", "dark"),
            ("tricky", "a == b"),
            ("empty", ""),
            ("post", ""),
            ("post.title", "Hi"),
        ])
    }

    fn ev(src: &str) -> Value {
        eval(src, &scope()).unwrap()
    }

    #[test]
    fn arithmetic_has_precedence() {
        assert_eq!(ev("2 + 3 * 4").to_string(), "14");
        assert_eq!(ev("(2 + 3) * 4").to_string(), "20");
        assert_eq!(ev("$count * 2 - 1").to_string(), "5");
        assert_eq!(ev("7 / 2").to_string(), "3.5");
        assert_eq!(ev("-$count").to_string(), "-3");
    }

    #[test]
    fn comparisons_and_logic() {
        assert!(ev("$count > 2").truthy());
        assert!(ev("$theme == dark").truthy());
        assert!(ev("$theme != light and not $empty").truthy());
        assert!(!ev("$count < 2 or $empty").truthy());
        assert!(ev("contains($name, or)").truthy());
        assert!(ev("starts-with($name, Wo) and ends-with($name, ld)").truthy());
        assert!(ev("uppercase($name) == WORLD").truthy());
        assert!(ev("#3b82f6 != red").truthy());
    }

    #[test]
    fn variable_values_are_never_reparsed() {
        // The value contains `==`, but it's just a string.
        assert!(ev("$tricky == \"a == b\"").truthy());
        assert!(ev("$tricky").truthy());
    }

    #[test]
    fn strings_and_if() {
        assert_eq!(ev("\"Hi $name!\"").to_string(), "Hi World!");
        assert_eq!(ev("if($count > 2, big, small)").to_string(), "big");
        assert_eq!(ev("${name}").to_string(), "World");
        assert_eq!(ev("${$count + 1} * 2").to_string(), "8");
    }

    #[test]
    fn only_the_branch_taken_is_evaluated() {
        let zero = Map::new(&[("n", "0"), ("on", "true")]);
        let ev0 = |src: &str| eval(src, &zero).map(|v| v.to_string());
        assert_eq!(ev0("if($n != 0, 10 / $n, 0)"), Ok("0".into()));
        assert_eq!(ev0("if($n == 0, none, 10 / $n)"), Ok("none".into()));
        assert_eq!(ev0("if($on, 24)"), Ok("24".into()));
        assert_eq!(ev0("if(not $on, 24)"), Ok("".into()));
        assert_eq!(
            ev0("if($on, \"#10b981\", \"var(--muted)\")"),
            Ok("#10b981".into())
        );
        // and / or stop at the side that decides
        assert_eq!(ev0("$n != 0 and 10 / $n > 1"), Ok("false".into()));
        assert_eq!(ev0("$n == 0 or 10 / $n > 1"), Ok("true".into()));
        assert_eq!(ev0("$on or darken(red, 10)"), Ok("true".into()));
        // The branch not taken is still read: its syntax, function names and
        // argument counts are checked; its variables and values are not
        assert_eq!(ev0("if($on, 1, $undefined)"), Ok("1".into()));
        assert!(eval("if($on, 1, (2)", &zero).is_err());
        assert!(eval("if($on, 1, nope(2))", &zero).is_err());
        assert!(eval("if($on, 1, uppercase(a, b))", &zero).is_err());
        // if() itself takes 2 or 3 arguments
        assert!(eval("if($on)", &zero).is_err());
        assert!(eval("if($on, 1, 2, 3)", &zero).is_err());
        // Something else after the condition is what is reported
        let error = eval("if($n.x, 1, 2)", &zero).unwrap_err().to_string();
        assert!(error.contains("`.x`"), "{}", error);
        // The taken branch still reports its errors
        assert!(eval("if($n == 0, 10 / $n, 0)", &zero).is_err());
        assert!(eval("$n == 0 and 10 / $n > 1", &zero).is_err());
    }

    #[test]
    fn names_and_fields() {
        assert_eq!(ev("$post.title").to_string(), "Hi");
        // A field the record doesn't have is empty
        assert_eq!(ev("default($post.draft, no)").to_string(), "no");
        assert_eq!(
            eval("$missing == 1", &scope()),
            Err(Error::Undefined {
                name: "missing".into(),
                offset: 0
            })
        );
        assert_eq!(
            eval("\"a $nope\"", &scope()),
            Err(Error::Undefined {
                name: "nope".into(),
                offset: 3
            })
        );
    }

    #[test]
    fn functions() {
        assert_eq!(ev("uppercase($name)").to_string(), "WORLD");
        assert_eq!(ev("length($name) + 1").to_string(), "6");
        assert_eq!(ev("truncate($name, 2)").to_string(), "Wo...");
        assert_eq!(ev("replace($name, o, 0)").to_string(), "W0rld");
        assert_eq!(ev("default($empty, none)").to_string(), "none");
        assert_eq!(ev("darken(#ffffff, 50)").to_string(), "#808080");
        assert_eq!(ev("mix(#000000, #ffffff, 50)").to_string(), "#808080");
        assert_eq!(ev("alpha(#3b82f6, 0.5)").to_string(), "#3b82f67f");
    }

    #[test]
    fn lists() {
        assert_eq!(ev("length($tags)").to_string(), "2");
        assert!(ev("contains($tags, \"web dev\")").truthy());
        assert!(!ev("contains($tags, web)").truthy());
        assert_eq!(ev("$tags").to_string(), "rust, web dev");
    }

    #[test]
    fn errors_are_reported() {
        let none = Map::new(&[]);
        assert!(eval("dark * 2", &none).is_err());
        assert!(eval("1 +", &none).is_err());
        assert!(eval("(1", &none).is_err());
        assert!(eval("1 / 0", &none).is_err());
        assert!(eval("nope(1)", &none).is_err());
        assert!(eval("uppercase(a, b)", &none).is_err());
        assert!(eval("darken(red, 10)", &none).is_err());
    }
}
