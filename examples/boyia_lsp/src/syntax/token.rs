use super::span::Span;

pub const BOYIA_KEYWORDS: &[&str] = &[
    "var", "fun", "class", "extends", "prop", "async", "await", "return", "if", "elif", "else",
    "do", "while", "for", "break", "require", "new", "null",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    Var,
    Fun,
    Class,
    If,
    Elif,
    Else,
    Do,
    While,
    For,
    Break,
    Extends,
    Prop,
    Return,
    Async,
    Await,
}

impl Keyword {
    pub fn from_ident(s: &str) -> Option<Keyword> {
        match s {
            "var" => Some(Keyword::Var),
            "fun" => Some(Keyword::Fun),
            "class" => Some(Keyword::Class),
            "if" => Some(Keyword::If),
            "elif" => Some(Keyword::Elif),
            "else" => Some(Keyword::Else),
            "do" => Some(Keyword::Do),
            "while" => Some(Keyword::While),
            "for" => Some(Keyword::For),
            "break" => Some(Keyword::Break),
            "extends" => Some(Keyword::Extends),
            "prop" => Some(Keyword::Prop),
            "return" => Some(Keyword::Return),
            "async" => Some(Keyword::Async),
            "await" => Some(Keyword::Await),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident,
    Keyword(Keyword),
    Number,
    Real,
    String,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Assign,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Not,
    Semi,
    Comma,
    Dot,
    Colon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Eof,
    Invalid(String),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub lexeme: String,
}

impl Token {
    pub fn is_keyword(&self, kw: Keyword) -> bool {
        matches!(self.kind, TokenKind::Keyword(k) if k == kw)
    }

    pub fn is_ident(&self) -> bool {
        matches!(self.kind, TokenKind::Ident)
    }

    pub fn is_semi(&self) -> bool {
        matches!(self.kind, TokenKind::Semi)
    }
}
