//! Tokenizer for the `.arch` DSL. `#` starts a comment to end of line.

use metaarch_spec::Span;

use crate::ParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Int(u64),
    /// A route path: `/` followed by path characters (`/orders/count`).
    Path(String),
    /// A raw `↖ … ↗` code fragment, dedented (see [`dedent`]).
    Fragment(String),
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
            Tok::Path(path) => write!(f, "`{path}`"),
            Tok::Fragment(_) => write!(f, "a `↖ … ↗` fragment"),
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

/// Normalize a raw fragment: strip leading/trailing blank lines and the
/// common leading whitespace, so generators can re-indent it per target.
fn dedent(text: &str) -> String {
    if !text.contains('\n') {
        return text.trim().to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|l| !l.trim().is_empty()) else {
        return String::new();
    };
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .expect("a non-blank line exists");
    let lines = &lines[start..=end];
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                &l[indent.min(l.len())..]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
                '/' => {
                    let mut path = String::from('/');
                    while let Some(&d) = self.chars.peek() {
                        if d.is_ascii_alphanumeric() || d == '_' || d == '-' || d == '/' {
                            path.push(d);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    Tok::Path(path)
                }
                // A raw code fragment between quilt's arrow brackets. The
                // brackets nest (a fragment may itself quote), and the text
                // is carried opaquely — the target language parses it later.
                '↖' => {
                    let mut depth = 1u32;
                    let mut text = String::new();
                    loop {
                        let Some(c) = self.bump() else {
                            return Err(ParseError {
                                message: "unterminated `↖ … ↗` fragment".into(),
                                span,
                            });
                        };
                        match c {
                            '↖' => {
                                depth += 1;
                                text.push(c);
                            }
                            '↗' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                text.push(c);
                            }
                            c => text.push(c),
                        }
                    }
                    Tok::Fragment(dedent(&text))
                }
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
