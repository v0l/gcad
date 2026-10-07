use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Line {
    pub number: usize,
    pub text: String,
    pub label: Option<String>,
    pub op: String,
    pub positional: Vec<String>,
    pub named: Vec<(String, String)>,
}

pub fn parse_program(source: &str) -> Result<Vec<Line>> {
    source
        .lines()
        .enumerate()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(index, text)| parse_line(index + 1, text))
        .collect()
}

fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn words(number: usize, text: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if quoted {
        bail!("line {number}: a quote is not closed");
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

pub fn parse_line(number: usize, text: &str) -> Result<Line> {
    let words = words(number, text)?;
    let mut words = words.iter().map(String::as_str).peekable();
    let label = match words.peek() {
        Some(word) if word.ends_with(':') => {
            let label = word.trim_end_matches(':');
            if !is_identifier(label) {
                bail!("line {number}: `{label}` is not a valid label");
            }
            words.next();
            Some(label.to_string())
        }
        _ => None,
    };
    let op = words
        .next()
        .ok_or_else(|| anyhow!("line {number}: label without an operation"))?
        .to_string();
    if !is_identifier(&op) {
        bail!("line {number}: `{op}` is not an operation name");
    }
    let (named, positional): (Vec<_>, Vec<_>) = words.partition(|word| {
        word.split_once('=')
            .is_some_and(|(key, _)| is_identifier(key))
    });
    Ok(Line {
        number,
        text: text.trim().to_string(),
        label,
        op,
        positional: positional.into_iter().map(str::to_string).collect(),
        named: named
            .into_iter()
            .map(|word| {
                let (key, value) = word.split_once('=').expect("partitioned on '='");
                (key.to_string(), value.to_string())
            })
            .collect(),
    })
}

pub type Scope = HashMap<String, f64>;

pub fn eval(expression: &str, scope: &Scope) -> Result<f64> {
    let tokens = tokenize(expression)?;
    let mut parser = ExprParser {
        tokens: &tokens,
        position: 0,
        scope,
    };
    let value = parser.comparison()?;
    if parser.position != tokens.len() {
        bail!(
            "unexpected `{}` in `{expression}`",
            tokens[parser.position].describe()
        );
    }
    Ok(value)
}

pub fn eval_point(text: &str, scope: &Scope) -> Result<(f64, f64)> {
    let parts = split_top_level(text, ',');
    match parts.as_slice() {
        [x, y] => Ok((eval(x, scope)?, eval(y, scope)?)),
        _ => bail!("`{text}` is not a point, write it as x,y"),
    }
}

fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut depth = 0i32;
    let mut start = 0;
    let mut parts = Vec::new();
    text.char_indices().for_each(|(i, c)| match c {
        '(' => depth += 1,
        ')' => depth -= 1,
        c if c == separator && depth == 0 => {
            parts.push(&text[start..i]);
            start = i + c.len_utf8();
        }
        _ => {}
    });
    parts.push(&text[start..]);
    parts
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Symbol(char),
    Compare(String),
}

impl Token {
    fn describe(&self) -> String {
        match self {
            Token::Number(n) => n.to_string(),
            Token::Name(name) => name.clone(),
            Token::Symbol(c) => c.to_string(),
            Token::Compare(op) => op.clone(),
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                i += 1;
                if i < chars.len() && (chars[i] == '-' || chars[i] == '+') {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let literal: String = chars[start..i].iter().collect();
            tokens.push(Token::Number(
                literal
                    .parse()
                    .map_err(|_| anyhow!("`{literal}` is not a number"))?,
            ));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token::Name(chars[start..i].iter().collect()));
        } else if "+-*/()".contains(c) {
            tokens.push(Token::Symbol(c));
            i += 1;
        } else if "<>=!".contains(c) {
            let pair = chars.get(i + 1) == Some(&'=');
            let op: String = if pair {
                [c, '='].iter().collect()
            } else {
                c.to_string()
            };
            if op == "=" || op == "!" {
                bail!("`{op}` is not an operator; compare with ==, !=, <, >, <= or >=");
            }
            i += op.len();
            tokens.push(Token::Compare(op));
        } else {
            bail!("unexpected `{c}` in `{text}`");
        }
    }
    Ok(tokens)
}

struct ExprParser<'a> {
    tokens: &'a [Token],
    position: usize,
    scope: &'a Scope,
}

impl ExprParser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn comparison(&mut self) -> Result<f64> {
        let left = self.sum()?;
        let Some(Token::Compare(op)) = self.peek().cloned() else {
            return Ok(left);
        };
        self.position += 1;
        let right = self.sum()?;
        let holds = match op.as_str() {
            "<" => left < right,
            ">" => left > right,
            "<=" => left <= right,
            ">=" => left >= right,
            "==" => (left - right).abs() <= 1.0e-12 * left.abs().max(right.abs()).max(1.0),
            _ => (left - right).abs() > 1.0e-12 * left.abs().max(right.abs()).max(1.0),
        };
        Ok(if holds { 1.0 } else { 0.0 })
    }

    fn sum(&mut self) -> Result<f64> {
        let mut value = self.product()?;
        while let Some(Token::Symbol(c @ ('+' | '-'))) = self.peek().cloned() {
            self.position += 1;
            let rhs = self.product()?;
            value = if c == '+' { value + rhs } else { value - rhs };
        }
        Ok(value)
    }

    fn product(&mut self) -> Result<f64> {
        let mut value = self.unary()?;
        while let Some(Token::Symbol(c @ ('*' | '/'))) = self.peek().cloned() {
            self.position += 1;
            let rhs = self.unary()?;
            value = if c == '*' { value * rhs } else { value / rhs };
        }
        Ok(value)
    }

    fn unary(&mut self) -> Result<f64> {
        match self.peek() {
            Some(Token::Symbol('-')) => {
                self.position += 1;
                Ok(-self.unary()?)
            }
            Some(Token::Symbol('+')) => {
                self.position += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<f64> {
        let token = self
            .peek()
            .cloned()
            .ok_or_else(|| anyhow!("expression ends early"))?;
        self.position += 1;
        match token {
            Token::Number(n) => Ok(n),
            Token::Name(name) => match name.as_str() {
                "pi" => Ok(std::f64::consts::PI),
                _ => self.scope.get(&name).copied().ok_or_else(|| {
                    anyhow!("`{name}` is not defined, set it with `let {name}=...`")
                }),
            },
            Token::Symbol('(') => {
                let value = self.comparison()?;
                match self.peek() {
                    Some(Token::Symbol(')')) => {
                        self.position += 1;
                        Ok(value)
                    }
                    _ => bail!("missing `)`"),
                }
            }
            other => bail!("unexpected `{}`", other.describe()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_with_label_positional_and_named() {
        let line = parse_line(3, "base: extrude t*2 draft=1.5").unwrap();
        assert_eq!(line.label.as_deref(), Some("base"));
        assert_eq!(line.op, "extrude");
        assert_eq!(line.positional, vec!["t*2"]);
        assert_eq!(line.named, vec![("draft".to_string(), "1.5".to_string())]);
    }

    #[test]
    fn selectors_with_equals_stay_positional() {
        let line = parse_line(1, "fillet 1 >Z&base.side").unwrap();
        assert_eq!(line.positional, vec!["1", ">Z&base.side"]);
    }

    #[test]
    fn expressions() {
        let scope = Scope::from([("w".to_string(), 40.0), ("t".to_string(), 3.0)]);
        assert_eq!(eval("w/2-t*(1+1)", &scope).unwrap(), 14.0);
        assert_eq!(eval("-w/4", &scope).unwrap(), -10.0);
        assert_eq!(eval("1.5e1", &scope).unwrap(), 15.0);
        assert_eq!(eval_point("w/2,-(t+1)", &scope).unwrap(), (20.0, -4.0));
        assert!(eval("q", &scope).unwrap_err().to_string().contains("let q"));
    }

    #[test]
    fn quoted_words_keep_their_spaces() {
        let line = parse_line(1, "text \"HELLO WORLD\" size=5").unwrap();
        assert_eq!(line.positional, vec!["HELLO WORLD"]);
    }

    #[test]
    fn comparisons() {
        let scope = Scope::from([("w".to_string(), 40.0)]);
        assert_eq!(eval("w>30", &scope).unwrap(), 1.0);
        assert_eq!(eval("w<=30", &scope).unwrap(), 0.0);
        assert_eq!(eval("w/2==20", &scope).unwrap(), 1.0);
        assert!(eval("w=40", &scope).is_err());
    }

    #[test]
    fn rejects_non_operations() {
        assert!(parse_line(1, "# a comment").is_err());
        assert!(parse_line(1, "base:").is_err());
    }
}
