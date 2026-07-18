//! Tokenizer for the `.arch` DSL. `#` starts a comment to end of line.

use metaarch_spec::Span;

use crate::ParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Int(u64),
    Colon,
    Comma,
    LBrace,
    RBrace,
    LParen,
    RParen,
    Eof,
}

impl std::fmt::Display for Tok {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tok::Ident(name) => write!(f, "`{name}`"),
            Tok::Int(n) => write!(f, "`{n}`"),
            Tok::Colon => write!(f, "`:`"),
            Tok::Comma => write!(f, "`,`"),
            Tok::LBrace => write!(f, "`{{`"),
            Tok::RBrace => write!(f, "`}}`"),
            Tok::LParen => write!(f, "`(`"),
            Tok::RParen => write!(f, "`)`"),
            Tok::Eof => write!(f, "end of file"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer {
            chars: src.chars().peekable(),
            line: 1,
            col: 1,
        }
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    pub fn lex(mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            // Skip whitespace and comments.
            match self.chars.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                    continue;
                }
                Some('#') => {
                    while let Some(&c) = self.chars.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                    continue;
                }
                _ => {}
            }

            let span = Span {
                line: self.line,
                col: self.col,
            };
            let Some(c) = self.bump() else {
                tokens.push(Token {
                    tok: Tok::Eof,
                    span,
                });
                return Ok(tokens);
            };

            let tok = match c {
                ':' => Tok::Colon,
                ',' => Tok::Comma,
                '{' => Tok::LBrace,
                '}' => Tok::RBrace,
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                c if c.is_ascii_digit() => {
                    let mut n = u64::from(c as u8 - b'0');
                    while let Some(&d) = self.chars.peek() {
                        let Some(digit) = d.to_digit(10) else { break };
                        self.bump();
                        n = n
                            .checked_mul(10)
                            .and_then(|n| n.checked_add(u64::from(digit)))
                            .ok_or_else(|| ParseError {
                                message: "number is too large".into(),
                                span,
                            })?;
                    }
                    Tok::Int(n)
                }
                c if c.is_alphabetic() || c == '_' => {
                    let mut name = String::from(c);
                    while let Some(&d) = self.chars.peek() {
                        if d.is_alphanumeric() || d == '_' {
                            name.push(d);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    Tok::Ident(name)
                }
                other => {
                    return Err(ParseError {
                        message: format!("unexpected character `{other}`"),
                        span,
                    });
                }
            };
            tokens.push(Token { tok, span });
        }
    }
}
