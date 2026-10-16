//! Tokenizer and Pratt parser for formulas.

use crate::{Error, Result, Value, blocked, is_identifier};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Literal(Value),
    Name(String),
    Array(Vec<Node>),
    Unary(String, Box<Node>),
    Binary(String, Box<Node>, Box<Node>),
    Member(Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    literal: Option<Value>,
}

const MAX_SOURCE: usize = 2048;
const MAX_TOKENS: usize = 512;
const MAX_NESTING: usize = 48;

fn precedence(op: &str) -> Option<u8> {
    Some(match op {
        "or" | "||" => 1,
        "and" | "&&" => 2,
        "==" | "=" | "!=" => 3,
        "<" | ">" | "<=" | ">=" => 4,
        "+" | "-" => 5,
        "*" | "/" | "%" | "mod" => 6,
        "^" => 7,
        _ => return None,
    })
}

fn tokenize(source: &str) -> Result<Vec<Token>> {
    if source.chars().count() > MAX_SOURCE {
        return Err(Error::new("Formula exceeds 2048 characters"));
    }
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = usize::from(chars.first() == Some(&'='));
    let op = |text: &str| Token {
        text: text.into(),
        literal: None,
    };
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if tokens.len() >= MAX_TOKENS {
            return Err(Error::new("Formula is too complex"));
        }
        if c == '"' || c == '\'' {
            let mut out = String::new();
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                let next = chars[i];
                i += 1;
                if next == c {
                    closed = true;
                    break;
                }
                if next == '\\' {
                    let escape = *chars
                        .get(i)
                        .ok_or_else(|| Error::new("Unterminated string"))?;
                    i += 1;
                    out.push(match escape {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        other => other,
                    });
                } else {
                    out.push(next);
                }
            }
            if !closed {
                return Err(Error::new("Unterminated string"));
            }
            tokens.push(Token {
                text: "string".into(),
                literal: Some(Value::Text(out)),
            });
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '.' {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            let value: f64 = text
                .parse()
                .map_err(|_| Error::new(format!("Invalid number {text}")))?;
            tokens.push(Token {
                text,
                literal: Some(Value::Number(value)),
            });
            continue;
        }
        let dollar = c == '$';
        let name_start = if dollar { i + 1 } else { i };
        if chars
            .get(name_start)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
        {
            let mut j = name_start;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let text: String = chars[i..j].iter().collect();
            if blocked(text.trim_start_matches('$')) {
                return Err(Error::new("Forbidden identifier"));
            }
            tokens.push(op(&text));
            i = j;
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        if ["==", "!=", "<=", ">=", "&&", "||"].contains(&two.as_str()) {
            tokens.push(op(&two));
            i += 2;
            continue;
        }
        if "+-*/%^<>=!()[].,;".contains(c) {
            tokens.push(op(&c.to_string()));
            i += 1;
            continue;
        }
        return Err(Error::new(format!("Unexpected character '{c}'")));
    }
    tokens.push(op("EOF"));
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    i: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> &str {
        &self.tokens[self.i].text
    }

    fn take(&mut self, text: &str) -> Result<()> {
        if self.peek() != text {
            return Err(Error::new(format!(
                "Expected '{text}', got '{}'",
                self.peek()
            )));
        }
        self.i += 1;
        Ok(())
    }

    fn next(&mut self) -> Result<Token> {
        let token = self
            .tokens
            .get(self.i)
            .cloned()
            .ok_or_else(|| Error::new("Unexpected end of formula"))?;
        self.i += 1;
        Ok(token)
    }

    fn list(&mut self, close: &str) -> Result<Vec<Node>> {
        let mut items = Vec::new();
        if self.peek() != close {
            loop {
                items.push(self.expression(0)?);
                if self.peek() != "," && self.peek() != ";" {
                    break;
                }
                self.i += 1;
            }
        }
        self.take(close)?;
        Ok(items)
    }

    fn expression(&mut self, min: u8) -> Result<Node> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(Error::new("Formula nesting limit exceeded"));
        }
        let token = self.next()?;
        let lower = token.text.to_lowercase();
        let mut left = if let Some(value) = token.literal {
            Node::Literal(value)
        } else if lower == "true" || lower == "false" || lower == "null" {
            Node::Literal(match lower.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => Value::Null,
            })
        } else if token.text == "(" {
            let inner = self.expression(0)?;
            self.take(")")?;
            inner
        } else if token.text == "[" {
            Node::Array(self.list("]")?)
        } else if ["-", "+", "!", "not"].contains(&token.text.as_str()) {
            Node::Unary(token.text, Box::new(self.expression(7)?))
        } else if token.text != "EOF"
            && token
                .text
                .trim_start_matches('$')
                .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        {
            if self.peek() == "(" {
                self.i += 1;
                Node::Call(token.text, self.list(")")?)
            } else {
                Node::Name(token.text)
            }
        } else {
            return Err(Error::new(format!(
                "Expected a value, got '{}'",
                token.text
            )));
        };
        loop {
            if self.peek() == "." {
                self.i += 1;
                let key = self.next()?.text;
                if !is_identifier(&key) || blocked(&key) {
                    return Err(Error::new("Invalid field"));
                }
                left = Node::Member(Box::new(left), Box::new(Node::Literal(Value::Text(key))));
                continue;
            }
            if self.peek() == "[" {
                self.i += 1;
                let key = self.expression(0)?;
                self.take("]")?;
                left = Node::Member(Box::new(left), Box::new(key));
                continue;
            }
            let op = self.peek().to_owned();
            let Some(p) = precedence(&op) else { break };
            if p < min {
                break;
            }
            self.i += 1;
            // `^` is right-associative; everything else is left-associative.
            let right = self.expression(if op == "^" { p } else { p + 1 })?;
            left = Node::Binary(op, Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }
}

/// Parse a formula (a leading `=` is allowed).
pub fn parse(source: &str) -> Result<Node> {
    let mut parser = Parser {
        tokens: tokenize(source)?,
        i: 0,
        depth: 0,
    };
    let node = parser.expression(0)?;
    parser.take("EOF")?;
    Ok(node)
}
