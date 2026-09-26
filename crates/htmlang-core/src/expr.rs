//! Expressions: conditions (`@if $count > 2 and not $hidden`), computed
//! values (`@let gap = $base * 2`) and `if(cond, a, b)`.
//!
//! Expressions are tokenized and evaluated as a whole. Variables are looked
//! up while evaluating, never pasted into the text first, so a value that
//! happens to contain `==` or spaces can't change how an expression parses.
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
//! `if(cond, a, b)`, tests (`contains(s, x)`, `starts-with(s, x)`,
//! `ends-with(s, x)`), text (`uppercase`, `lowercase`, `capitalize`, `trim`,
//! `length`, `reverse`, `truncate(s, n)`, `replace(s, old, new)`,
//! `default(s, fallback)`) and colors (`lighten(c, pct)`, `darken(c, pct)`,
//! `alpha(c, a)`, `mix(c1, c2, pct)`). In text, `${expr}` interpolates an
//! expression.

use std::fmt;

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

/// Resolves a variable name (the text after `$`). Returns `None` for
/// undefined variables.
pub type Resolver<'a> = &'a dyn Fn(&str) -> Option<Value>;

/// Evaluate `src`. Undefined variables are empty strings.
pub fn eval(src: &str, resolve: Resolver) -> Result<Value, String> {
    let tokens = tokenize(src)?;
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        resolve,
    };
    let value = parser.expr()?;
    match parser.tokens.get(parser.pos) {
        None => Ok(value),
        Some(tok) => Err(format!("unexpected {} in `{}`", tok, src.trim())),
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

fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let is_word_char = |c: char| c.is_alphanumeric() || matches!(c, '#' | '.' | '_' | '-' | '%');
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            tokens.push(Token::LParen);
            i += 1;
        } else if c == ')' {
            tokens.push(Token::RParen);
            i += 1;
        } else if c == ',' {
            tokens.push(Token::Comma);
            i += 1;
        } else if c == '"' {
            let start = i + 1;
            let end = chars[start..]
                .iter()
                .position(|&c| c == '"')
                .map(|p| start + p)
                .ok_or_else(|| format!("unclosed string in `{}`", src.trim()))?;
            tokens.push(Token::Str(chars[start..end].iter().collect()));
            i = end + 1;
        } else if c == '$' {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && is_var_char(chars[end]) {
                end += 1;
            }
            while end > start && chars[end - 1] == '.' {
                end -= 1;
            }
            if end == start {
                return Err(format!("`$` without a variable name in `{}`", src.trim()));
            }
            tokens.push(Token::Var(chars[start..end].iter().collect()));
            i = end;
        } else if let Some(op) = OPERATORS
            .iter()
            .find(|op| chars[i..].iter().take(op.len()).copied().eq(op.chars()))
            // A `-` followed by a letter starts a word (`-webkit-box`);
            // inside words (`ease-in`) it is consumed by the word itself.
            .filter(|op| **op != "-" || !chars.get(i + 1).is_some_and(|c| c.is_alphabetic()))
        {
            tokens.push(Token::Op(op));
            i += op.len();
        } else if is_word_char(c) {
            let start = i;
            while i < chars.len() && is_word_char(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            tokens.push(match word.parse::<f64>() {
                Ok(n) if !word.ends_with('.') => Token::Num(n),
                _ => Token::Word(word),
            });
        } else {
            return Err(format!("unexpected `{}` in `{}`", c, src.trim()));
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    resolve: Resolver<'a>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
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

    fn expect(&mut self, token: Token) -> Result<(), String> {
        match self.peek() {
            Some(t) if *t == token => {
                self.pos += 1;
                Ok(())
            }
            Some(t) => Err(format!("expected {} but found {}", token, t)),
            None => Err(format!("expected {} at the end", token)),
        }
    }

    fn expr(&mut self) -> Result<Value, String> {
        let mut left = self.and()?;
        while self.eat_word("or") {
            let right = self.and()?;
            left = Value::Bool(left.truthy() || right.truthy());
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Value, String> {
        let mut left = self.not()?;
        while self.eat_word("and") {
            let right = self.not()?;
            left = Value::Bool(left.truthy() && right.truthy());
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Value, String> {
        if self.eat_word("not") {
            return Ok(Value::Bool(!self.not()?.truthy()));
        }
        self.compare()
    }

    fn compare(&mut self) -> Result<Value, String> {
        let left = self.sum()?;
        let Some(op) = self.eat_op(&["==", "!=", "<=", ">=", "<", ">"]) else {
            return Ok(left);
        };
        let right = self.sum()?;
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

    fn sum(&mut self) -> Result<Value, String> {
        let mut left = self.product()?;
        while let Some(op) = self.eat_op(&["+", "-"]) {
            let right = self.product()?;
            left = arithmetic(op, &left, &right)?;
        }
        Ok(left)
    }

    fn product(&mut self) -> Result<Value, String> {
        let mut left = self.unary()?;
        while let Some(op) = self.eat_op(&["*", "/", "%"]) {
            let right = self.unary()?;
            left = arithmetic(op, &left, &right)?;
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Value, String> {
        if self.eat_op(&["-"]).is_some() {
            let value = self.unary()?;
            return value
                .as_num()
                .map(|n| Value::Num(-n))
                .ok_or_else(|| format!("cannot negate `{}`", value));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Value, String> {
        let token = self
            .peek()
            .cloned()
            .ok_or_else(|| "expected a value at the end".to_string())?;
        self.pos += 1;
        match token {
            Token::Num(n) => Ok(Value::Num(n)),
            Token::Str(s) => Ok(Value::Str(interpolate(&s, self.resolve))),
            Token::Var(name) => Ok((self.resolve)(&name).unwrap_or(Value::Str(String::new()))),
            Token::Word(w) if w == "true" => Ok(Value::Bool(true)),
            Token::Word(w) if w == "false" => Ok(Value::Bool(false)),
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
                call(&name, args)
            }
            Token::Word(w) => Ok(Value::Str(w)),
            Token::LParen => {
                let value = self.expr()?;
                self.expect(Token::RParen)?;
                Ok(value)
            }
            other => Err(format!("unexpected {}", other)),
        }
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

fn is_var_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// Call a built-in function.
fn call(name: &str, args: Vec<Value>) -> Result<Value, String> {
    let text = |i: usize| args.get(i).map(|v| v.to_string()).unwrap_or_default();
    let num = |i: usize| {
        args.get(i)
            .and_then(Value::as_num)
            .ok_or_else(|| format!("{}() needs a number as argument {}", name, i + 1))
    };
    let arity = |n: usize| {
        if args.len() == n {
            Ok(())
        } else {
            Err(format!("{}() takes {} argument{}", name, n, if n == 1 { "" } else { "s" }))
        }
    };
    let color = |rgb: (u8, u8, u8)| Value::Str(format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2));
    Ok(match name {
        "if" => {
            arity(3)?;
            let mut args = args;
            let otherwise = args.pop().unwrap();
            let then = args.pop().unwrap();
            if args[0].truthy() { then } else { otherwise }
        }
        "contains" => {
            arity(2)?;
            Value::Bool(match &args[0] {
                Value::List(items) => items.contains(&text(1)),
                other => other.to_string().contains(&text(1)),
            })
        }
        "starts-with" => { arity(2)?; Value::Bool(text(0).starts_with(&text(1))) }
        "ends-with" => { arity(2)?; Value::Bool(text(0).ends_with(&text(1))) }
        "uppercase" => { arity(1)?; Value::Str(text(0).to_uppercase()) }
        "lowercase" => { arity(1)?; Value::Str(text(0).to_lowercase()) }
        "capitalize" => {
            arity(1)?;
            let s = text(0);
            let mut chars = s.chars();
            Value::Str(match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => String::new(),
            })
        }
        "trim" => { arity(1)?; Value::Str(text(0).trim().to_string()) }
        "length" => {
            arity(1)?;
            Value::Num(match &args[0] {
                Value::List(items) => items.len(),
                other => other.to_string().chars().count(),
            } as f64)
        }
        "reverse" => { arity(1)?; Value::Str(text(0).chars().rev().collect()) }
        "truncate" => {
            arity(2)?;
            let (s, n) = (text(0), num(1)? as usize);
            Value::Str(if s.chars().count() > n {
                format!("{}...", s.chars().take(n).collect::<String>())
            } else {
                s
            })
        }
        "replace" => { arity(3)?; Value::Str(text(0).replace(&text(1), &text(2))) }
        "default" => {
            arity(2)?;
            let s = text(0);
            Value::Str(if s.is_empty() { text(1) } else { s })
        }
        "lighten" | "darken" | "alpha" | "mix" => {
            let expected = if name == "mix" { 3 } else { 2 };
            arity(expected)?;
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


/// Replace `$name` references inside a string literal.
fn interpolate(s: &str, resolve: Resolver) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after.find(|c: char| !is_var_char(c)).unwrap_or(after.len());
        if end == 0 {
            out.push('$');
        } else {
            out.push_str(&resolve(&after[..end]).map(|v| v.to_string()).unwrap_or_default());
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(src: &str) -> Value {
        let resolve = |name: &str| {
            let text = |s: &str| Some(Value::Str(s.to_string()));
            match name {
                "tags" => Some(Value::List(vec!["rust".into(), "web dev".into()])),
                "count" => text("3"),
                "name" => text("World"),
                "theme" => text("dark"),
                "tricky" => text("a == b"),
                "empty" => text(""),
                _ => None,
            }
        };
        eval(src, &resolve).unwrap()
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
        assert_eq!(ev("$missing").to_string(), "");
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
        let none = |_: &str| None;
        assert!(eval("dark * 2", &none).is_err());
        assert!(eval("1 +", &none).is_err());
        assert!(eval("(1", &none).is_err());
        assert!(eval("1 / 0", &none).is_err());
        assert!(eval("nope(1)", &none).is_err());
        assert!(eval("uppercase(a, b)", &none).is_err());
        assert!(eval("darken(red, 10)", &none).is_err());
    }
}
