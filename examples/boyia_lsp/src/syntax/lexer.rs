use super::span::Span;
use super::token::{Keyword, Token, TokenKind};

pub struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
            col: 0,
        }
    }

    pub fn tokenize(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token();
            let is_eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        tokens
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace_and_comments();
        let start = self.pos;
        if self.at_end() {
            return self.make_token(TokenKind::Eof, start, start, "");
        }

        let b = self.peek_byte();
        if b.is_ascii_alphabetic() || b == b'_' {
            return self.read_ident_or_keyword(start);
        }
        if b.is_ascii_digit() {
            return self.read_number(start);
        }
        if b == b'"' {
            return self.read_string(start);
        }

        match b {
            b'+' => self.single(TokenKind::Plus, start),
            b'-' => self.single(TokenKind::Minus, start),
            b'*' => self.single(TokenKind::Star, start),
            b'/' => self.single(TokenKind::Slash, start),
            b'%' => self.single(TokenKind::Percent, start),
            b'^' => self.single(TokenKind::Caret, start),
            b';' => self.single(TokenKind::Semi, start),
            b',' => self.single(TokenKind::Comma, start),
            b'.' => self.single(TokenKind::Dot, start),
            b':' => self.single(TokenKind::Colon, start),
            b'(' => self.single(TokenKind::LParen, start),
            b')' => self.single(TokenKind::RParen, start),
            b'[' => self.single(TokenKind::LBracket, start),
            b']' => self.single(TokenKind::RBracket, start),
            b'{' => self.single(TokenKind::LBrace, start),
            b'}' => self.single(TokenKind::RBrace, start),
            b'=' => {
                if self.peek_byte_at(1) == b'=' {
                    self.advance();
                    self.advance();
                    self.make_token(TokenKind::EqEq, start, self.pos, "==")
                } else {
                    self.single(TokenKind::Assign, start)
                }
            }
            b'!' => {
                if self.peek_byte_at(1) == b'=' {
                    self.advance();
                    self.advance();
                    self.make_token(TokenKind::Ne, start, self.pos, "!=")
                } else {
                    self.single(TokenKind::Not, start)
                }
            }
            b'<' => {
                if self.peek_byte_at(1) == b'=' {
                    self.advance();
                    self.advance();
                    self.make_token(TokenKind::Le, start, self.pos, "<=")
                } else {
                    self.single(TokenKind::Lt, start)
                }
            }
            b'>' => {
                if self.peek_byte_at(1) == b'=' {
                    self.advance();
                    self.advance();
                    self.make_token(TokenKind::Ge, start, self.pos, ">=")
                } else {
                    self.single(TokenKind::Gt, start)
                }
            }
            b'&' => {
                if self.peek_byte_at(1) == b'&' {
                    self.advance();
                    self.advance();
                    self.make_token(TokenKind::AndAnd, start, self.pos, "&&")
                } else {
                    self.advance();
                    self.make_token(
                        TokenKind::Invalid("&".into()),
                        start,
                        self.pos,
                        "&",
                    )
                }
            }
            b'|' => {
                if self.peek_byte_at(1) == b'|' {
                    self.advance();
                    self.advance();
                    self.make_token(TokenKind::OrOr, start, self.pos, "||")
                } else {
                    self.advance();
                    self.make_token(
                        TokenKind::Invalid("|".into()),
                        start,
                        self.pos,
                        "|",
                    )
                }
            }
            _ => {
                self.advance();
                self.make_token(
                    TokenKind::Invalid(String::from(b as char)),
                    start,
                    self.pos,
                    &self.source[start..self.pos],
                )
            }
        }
    }

    fn single(&mut self, kind: TokenKind, start: usize) -> Token {
        self.advance();
        self.make_token(kind, start, self.pos, &self.source[start..self.pos])
    }

    fn read_ident_or_keyword(&mut self, start: usize) -> Token {
        while !self.at_end() {
            let b = self.peek_byte();
            if b == b'_' || b.is_ascii_alphanumeric() {
                self.advance();
            } else {
                break;
            }
        }
        let lexeme = &self.source[start..self.pos];
        let kind = Keyword::from_ident(lexeme)
            .map(TokenKind::Keyword)
            .unwrap_or(TokenKind::Ident);
        self.make_token(kind, start, self.pos, lexeme)
    }

    fn read_number(&mut self, start: usize) -> Token {
        let mut dot_count = 0u8;
        while !self.at_end() {
            let b = self.peek_byte();
            if b.is_ascii_digit() {
                self.advance();
            } else if b == b'.' && dot_count == 0 {
                dot_count += 1;
                self.advance();
            } else {
                break;
            }
        }
        let kind = if dot_count == 0 {
            TokenKind::Number
        } else {
            TokenKind::Real
        };
        self.make_token(kind, start, self.pos, &self.source[start..self.pos])
    }

    fn read_string(&mut self, start: usize) -> Token {
        self.advance(); // opening "
        while !self.at_end() {
            let b = self.peek_byte();
            if b == b'"' {
                self.advance();
                break;
            }
            if b == b'\r' {
                break;
            }
            self.advance();
        }
        self.make_token(
            TokenKind::String,
            start,
            self.pos,
            &self.source[start..self.pos],
        )
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            if self.at_end() {
                return;
            }
            let b = self.peek_byte();
            if b == b' ' || b == b'\t' || b == b'\r' {
                self.advance();
                continue;
            }
            if b == b'\n' {
                self.advance();
                self.line += 1;
                self.col = 0;
                continue;
            }
            if b == b'/' && self.peek_byte_at(1) == b'/' {
                while !self.at_end() && self.peek_byte() != b'\n' {
                    self.advance();
                }
                continue;
            }
            if b == b'/' && self.peek_byte_at(1) == b'*' {
                self.advance();
                self.advance();
                while !self.at_end() {
                    if self.peek_byte() == b'*' && self.peek_byte_at(1) == b'/' {
                        self.advance();
                        self.advance();
                        break;
                    }
                    if self.peek_byte() == b'\n' {
                        self.line += 1;
                        self.col = 0;
                    }
                    self.advance();
                }
                continue;
            }
            break;
        }
    }

    fn make_token(&self, kind: TokenKind, start: usize, end: usize, lexeme: &str) -> Token {
        Token {
            kind,
            span: Span::new(start, end),
            lexeme: lexeme.to_string(),
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn peek_byte(&self) -> u8 {
        self.bytes.get(self.pos).copied().unwrap_or(0)
    }

    fn peek_byte_at(&self, offset: usize) -> u8 {
        self.bytes.get(self.pos + offset).copied().unwrap_or(0)
    }

    fn advance(&mut self) {
        if !self.at_end() {
            self.pos += 1;
            self.col += 1;
        }
    }
}
