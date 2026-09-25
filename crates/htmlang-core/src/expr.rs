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
//! compare := concat (("==" | "!=" | "<" | ">" | "<=" | ">=" | "contains"
//!                     | "starts-with" | "ends-with") concat)?
//! concat  := sum ("~" sum)*
//! sum     := product (("+" | "-") product)*
//! product := unary (("*" | "/" | "%") unary)*
//! unary   := "-" unary | primary
//! primary := NUMBER | "STRING" | $VAR | WORD | true | false
//!          | if(expr, expr, expr) | "(" expr ")"
//! ```
//!
//! A bare word (`dark`, `red`, `#3b82f6`, `10px`) is a string.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f64),
    Str(String),
    Bool(bool),
}

impl Value {
    /// Empty strings, `false`, `0` and `"false"` / `"0"` are false.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty() && s != "false" && s != "0",
        }
    }

    fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Str(s) => s.trim().parse().ok(),
            Value::Bool(_) => None,
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
        }
    }
}

/// Resolves a variable reference: the text after `$`, including any
/// filters (`name|uppercase`). Returns `None` for undefined variables.
pub type Resolver<'a> = &'a dyn Fn(&str) -> Option<String>;

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

const OPERATORS: &[&str] = &["==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "~"];

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
            // $name, with optional |filter:args chains
            let start = i + 1;
            let mut end = start;
            while end < chars.len()
                && !chars[end].is_whitespace()
                && !matches!(chars[end], '(' | ')' | ',' | '"')
            {
                end += 1;
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
        let left = self.concat()?;
        let op = if let Some(op) = self.eat_op(&["==", "!=", "<=", ">=", "<", ">"]) {
            op
        } else if self.eat_word("contains") {
            "contains"
        } else if self.eat_word("starts-with") {
            "starts-with"
        } else if self.eat_word("ends-with") {
            "ends-with"
        } else {
            return Ok(left);
        };
        let right = self.concat()?;
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
            ("contains", _) => l.contains(&r),
            ("starts-with", _) => l.starts_with(&r),
            ("ends-with", _) => l.ends_with(&r),
            _ => unreachable!(),
        }))
    }

    fn concat(&mut self) -> Result<Value, String> {
        let mut left = self.sum()?;
        while self.eat_op(&["~"]).is_some() {
            let right = self.sum()?;
            left = Value::Str(format!("{}{}", left, right));
        }
        Ok(left)
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
            Token::Var(name) => Ok(Value::Str((self.resolve)(&name).unwrap_or_default())),
            Token::Word(w) if w == "true" => Ok(Value::Bool(true)),
            Token::Word(w) if w == "false" => Ok(Value::Bool(false)),
            Token::Word(w) if w == "if" && self.peek() == Some(&Token::LParen) => {
                self.pos += 1;
                let condition = self.expr()?;
                self.expect(Token::Comma)?;
                let then = self.expr()?;
                self.expect(Token::Comma)?;
                let otherwise = self.expr()?;
                self.expect(Token::RParen)?;
                Ok(if condition.truthy() { then } else { otherwise })
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

/// Replace `$name` references inside a string literal.
fn interpolate(s: &str, resolve: Resolver) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '|' | ':')))
            .unwrap_or(after.len());
        if end == 0 {
            out.push('$');
        } else {
            out.push_str(&resolve(&after[..end]).unwrap_or_default());
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
        let resolve = |name: &str| match name {
            "count" => Some("3".to_string()),
            "name" => Some("World".to_string()),
            "theme" => Some("dark".to_string()),
            "tricky" => Some("a == b".to_string()),
            "empty" => Some(String::new()),
            "name|uppercase" => Some("WORLD".to_string()),
            _ => None,
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
        assert!(ev("$name contains or").truthy());
        assert!(ev("$name|uppercase == WORLD").truthy());
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
        assert_eq!(ev("\"Hello \" ~ $name").to_string(), "Hello World");
        assert_eq!(ev("\"Hi $name!\"").to_string(), "Hi World!");
        assert_eq!(ev("if($count > 2, big, small)").to_string(), "big");
        assert_eq!(ev("$missing").to_string(), "");
    }

    #[test]
    fn errors_are_reported() {
        let none = |_: &str| None;
        assert!(eval("dark * 2", &none).is_err());
        assert!(eval("1 +", &none).is_err());
        assert!(eval("(1", &none).is_err());
        assert!(eval("1 / 0", &none).is_err());
    }
}
