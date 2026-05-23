use super::ast::*;
use super::lexer::Lexer;
use super::span::Span;
use super::token::{Keyword, Token, TokenKind};

#[derive(Clone, Debug)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ParseResult {
    pub program: Program,
    pub errors: Vec<ParseError>,
}

pub fn parse(source: &str) -> ParseResult {
    let tokens = Lexer::new(source).tokenize();
    let mut parser = Parser {
        source,
        tokens,
        index: 0,
        errors: Vec::new(),
    };
    let program = parser.parse_program();
    ParseResult {
        program,
        errors: parser.errors,
    }
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    index: usize,
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    fn parse_program(&mut self) -> Program {
        let start = self.current_span().start;
        let mut items = Vec::new();
        while !self.at_end() {
            self.skip_toplevel_brace_fragments();
            if self.at_end() {
                break;
            }
            if let Some(item) = self.parse_top_level_item() {
                items.push(item);
            } else {
                self.synchronize_toplevel();
            }
        }
        Program {
            items,
            span: Span::new(start, self.source.len()),
        }
    }

    fn parse_top_level_item(&mut self) -> Option<TopLevelItem> {
        let start = self.current_span();
        match &self.current().kind {
            TokenKind::Keyword(Keyword::Var) => {
                let decl = self.parse_var_decl(start);
                Some(TopLevelItem::VarDecl(decl))
            }
            TokenKind::Keyword(Keyword::Fun) => {
                let fun = self.parse_fun_decl(FunKind::Function, start);
                Some(TopLevelItem::FunDecl(fun))
            }
            TokenKind::Keyword(Keyword::Class) => {
                let class = self.parse_class_decl(start);
                Some(TopLevelItem::ClassDecl(class))
            }
            TokenKind::LBrace => {
                self.error("'{' at top level is ignored", start);
                self.skip_balanced_brace();
                None
            }
            _ => {
                let stmt = self.parse_expr_stmt(start)?;
                Some(TopLevelItem::ExprStmt(stmt))
            }
        }
    }

    fn parse_var_decl(&mut self, start: Span) -> VarDecl {
        self.advance(); // var
        let mut names = Vec::new();
        loop {
            if !self.current().is_ident() {
                self.error("identifier expected after 'var'", self.current_span());
                break;
            }
            let name = self.current().lexeme.clone();
            let name_span = self.current_span();
            names.push(NameBinding { name, name_span });
            self.advance();
            self.consume_expression_fragment();
            if !self.match_token(TokenKind::Comma) {
                break;
            }
        }
        self.expect_semi("'var' declaration");
        VarDecl {
            names,
            span: start.merge(self.prev_span()),
        }
    }

    fn parse_fun_decl(&mut self, kind: FunKind, start: Span) -> FunDecl {
        self.advance(); // fun / already consumed async+fun in prop
        let (name, name_span) = self.expect_ident("function name");
        let params = self.parse_param_list();
        let body = self.parse_block_body();
        let body_span = body.span;
        FunDecl {
            name,
            name_span,
            params,
            body,
            kind,
            span: start.merge(body_span),
        }
    }

    fn parse_class_decl(&mut self, start: Span) -> ClassDecl {
        self.advance(); // class
        let (name, name_span) = self.expect_ident("class name");
        let extends = if self.current().is_keyword(Keyword::Extends) {
            self.advance();
            let (ext_name, ext_span) = self.expect_ident("base class name");
            Some((ext_name, ext_span))
        } else {
            None
        };
        let members = self.parse_class_body();
        ClassDecl {
            name,
            name_span,
            extends,
            members,
            span: start.merge(self.prev_span()),
        }
    }

    fn parse_class_body(&mut self) -> Vec<ClassMember> {
        let mut members = Vec::new();
        self.expect_lbrace("class body");
        while !self.at_end() && !self.current_is(TokenKind::RBrace) {
            if let Some(member) = self.parse_class_member() {
                members.push(member);
            } else {
                self.synchronize_block();
            }
        }
        self.expect_rbrace("class body");
        let _ = self.match_token(TokenKind::Semi);
        members
    }

    fn parse_class_member(&mut self) -> Option<ClassMember> {
        let start = self.current_span();
        match &self.current().kind {
            TokenKind::Keyword(Keyword::Prop) => {
                self.advance();
                if self.current().is_keyword(Keyword::Fun) {
                    let fun = self.parse_fun_decl(FunKind::PropMethod, start);
                    return Some(ClassMember::Method(fun));
                }
                if self.current().is_keyword(Keyword::Async) {
                    self.advance();
                    if self.current().is_keyword(Keyword::Fun) {
                        let fun = self.parse_fun_decl(FunKind::AsyncPropMethod, start);
                        return Some(ClassMember::Method(fun));
                    }
                    self.error("'fun' expected after 'prop async'", self.current_span());
                    self.skip_statement();
                    return None;
                }
                if self.current().is_ident() {
                    let name = self.current().lexeme.clone();
                    let name_span = self.current_span();
                    self.advance();
                    self.consume_expression_fragment();
                    self.expect_semi("prop assignment");
                    return Some(ClassMember::PropAssign {
                        name,
                        name_span,
                        span: start.merge(self.prev_span()),
                    });
                }
                self.error("identifier or 'fun' expected after 'prop'", self.current_span());
                self.skip_statement();
                None
            }
            TokenKind::Keyword(Keyword::Fun) => {
                let fun = self.parse_fun_decl(FunKind::Method, start);
                Some(ClassMember::Method(fun))
            }
            TokenKind::Keyword(Keyword::Async) => {
                self.advance();
                if self.current().is_keyword(Keyword::Fun) {
                    let fun = self.parse_fun_decl(FunKind::AsyncPropMethod, start);
                    Some(ClassMember::Method(fun))
                } else {
                    self.error("'fun' expected after 'async'", self.current_span());
                    self.skip_statement();
                    None
                }
            }
            TokenKind::Keyword(Keyword::Var) => {
                // var inside class body — treat as statement, not member decl
                let _ = self.parse_block_stmt();
                None
            }
            _ => {
                let _ = self.parse_block_stmt();
                None
            }
        }
    }

    fn parse_block_body(&mut self) -> Block {
        let start = self.current_span();
        self.expect_lbrace("function body");
        let stmts = self.parse_block_stmts();
        self.expect_rbrace("function body");
        Block {
            stmts,
            span: start.merge(self.prev_span()),
        }
    }

    fn parse_block_stmts(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while !self.at_end() && !self.current_is(TokenKind::RBrace) {
            if let Some(stmt) = self.parse_block_stmt() {
                stmts.push(stmt);
            } else {
                self.synchronize_block();
            }
        }
        stmts
    }

    fn parse_block_stmt(&mut self) -> Option<Stmt> {
        let start = self.current_span();
        match &self.current().kind {
            TokenKind::Keyword(Keyword::Var) => {
                let decl = self.parse_var_decl(start);
                Some(Stmt::VarDecl(decl))
            }
            TokenKind::Keyword(Keyword::Fun) => {
                let fun = self.parse_fun_decl(FunKind::Function, start);
                Some(Stmt::FunDecl(fun))
            }
            TokenKind::Keyword(Keyword::Async) => {
                self.advance();
                if self.current().is_keyword(Keyword::Fun) {
                    let fun = self.parse_fun_decl(FunKind::AsyncFunction, start);
                    Some(Stmt::FunDecl(fun))
                } else {
                    self.error("'fun' expected after 'async'", self.current_span());
                    self.skip_statement();
                    None
                }
            }
            TokenKind::Keyword(Keyword::Return) => {
                self.advance();
                if !self.current().is_semi() && !self.at_end() {
                    self.consume_expression_fragment();
                }
                self.expect_semi("'return'");
                Some(Stmt::Return {
                    span: start.merge(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::If) | TokenKind::Keyword(Keyword::Elif) => {
                self.parse_if_like(start)
            }
            TokenKind::Keyword(Keyword::Else) => self.parse_else_block(start),
            TokenKind::Keyword(Keyword::While) => self.parse_while(start),
            TokenKind::Keyword(Keyword::Do) => self.parse_do_while(start),
            TokenKind::Keyword(Keyword::For) => self.parse_for(start),
            TokenKind::Keyword(Keyword::Break) => {
                self.advance();
                Some(Stmt::Break {
                    span: start.merge(self.current_span()),
                })
            }
            TokenKind::Keyword(Keyword::Await) => {
                self.advance();
                self.consume_expression_fragment();
                self.expect_semi("'await'");
                Some(Stmt::Await {
                    span: start.merge(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Prop) => {
                self.advance();
                if self.current().is_keyword(Keyword::Fun) || self.current().is_keyword(Keyword::Async) {
                    let _ = self.parse_class_member();
                    return None;
                }
                if self.current().is_ident() {
                    let name = self.current().lexeme.clone();
                    let name_span = self.current_span();
                    self.advance();
                    self.consume_expression_fragment();
                    self.expect_semi("prop assignment");
                    return Some(Stmt::PropAssign {
                        name,
                        name_span,
                        span: start.merge(self.prev_span()),
                    });
                }
                self.error("identifier or 'fun' expected after 'prop'", self.current_span());
                self.skip_statement();
                None
            }
            TokenKind::LBrace => {
                self.advance();
                let inner = self.parse_block_stmts();
                self.expect_rbrace("block");
                Some(Stmt::Block(Block {
                    stmts: inner,
                    span: start.merge(self.prev_span()),
                }))
            }
            _ => {
                let expr = self.parse_expr_stmt(start)?;
                Some(Stmt::ExprStmt(expr))
            }
        }
    }

    fn parse_if_like(&mut self, start: Span) -> Option<Stmt> {
        self.advance();
        self.consume_expression_fragment();
        self.expect_lbrace("'if' branch");
        self.skip_block_contents();
        self.expect_rbrace("'if' branch");
        Some(Stmt::If {
            span: start.merge(self.prev_span()),
        })
    }

    fn parse_else_block(&mut self, start: Span) -> Option<Stmt> {
        self.advance();
        self.expect_lbrace("'else' branch");
        self.skip_block_contents();
        self.expect_rbrace("'else' branch");
        Some(Stmt::If {
            span: start.merge(self.prev_span()),
        })
    }

    fn parse_while(&mut self, start: Span) -> Option<Stmt> {
        self.advance();
        self.consume_expression_fragment();
        self.expect_lbrace("'while' body");
        self.skip_block_contents();
        self.expect_rbrace("'while' body");
        Some(Stmt::While {
            span: start.merge(self.prev_span()),
        })
    }

    fn parse_do_while(&mut self, start: Span) -> Option<Stmt> {
        self.advance();
        self.expect_lbrace("'do' body");
        self.skip_block_contents();
        self.expect_rbrace("'do' body");
        if self.current().is_keyword(Keyword::While) {
            self.advance();
            self.consume_expression_fragment();
        }
        Some(Stmt::DoWhile {
            span: start.merge(self.prev_span()),
        })
    }

    fn parse_for(&mut self, start: Span) -> Option<Stmt> {
        self.advance();
        if !self.match_token(TokenKind::LParen) {
            self.error("'(' expected after 'for'", self.current_span());
            self.skip_statement();
            return None;
        }
        if self.current().is_keyword(Keyword::Var) {
            let _ = self.parse_var_decl(start);
        } else {
            self.consume_expression_fragment();
        }
        self.consume_expression_fragment();
        self.consume_expression_fragment();
        if !self.match_token(TokenKind::RParen) {
            self.error("')' expected in 'for'", self.current_span());
            self.skip_statement();
            return None;
        }
        self.expect_lbrace("'for' body");
        self.skip_block_contents();
        self.expect_rbrace("'for' body");
        Some(Stmt::For {
            span: start.merge(self.prev_span()),
        })
    }

    fn parse_expr_stmt(&mut self, start: Span) -> Option<ExprStmt> {
        let requires = self.consume_expression_with_requires();
        self.expect_semi("expression");
        Some(ExprStmt {
            requires,
            span: start.merge(self.prev_span()),
        })
    }

    fn parse_param_list(&mut self) -> Vec<String> {
        let mut params = Vec::new();
        if !self.match_token(TokenKind::LParen) {
            self.error("'(' expected before parameter list", self.current_span());
            return params;
        }
        while !self.at_end() && !self.current_is(TokenKind::RParen) {
            if self.current().is_ident() {
                params.push(self.current().lexeme.clone());
                self.advance();
            } else {
                self.error("parameter name expected", self.current_span());
                self.advance();
            }
            if !self.match_token(TokenKind::Comma) {
                break;
            }
        }
        if !self.match_token(TokenKind::RParen) {
            self.error("')' expected after parameter list", self.current_span());
        }
        params
    }

    fn consume_expression_with_requires(&mut self) -> Vec<RequireCall> {
        let mut requires = Vec::new();
        let mut depth_paren = 0i32;
        let mut depth_bracket = 0i32;
        let mut depth_brace = 0i32;

        while !self.at_end() {
            if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 {
                if self.current().is_semi() {
                    break;
                }
                if self.current().is_keyword(Keyword::Fun)
                    || self.current().is_keyword(Keyword::Async)
                {
                    self.skip_anonymous_function();
                    continue;
                }
                if self.current().is_ident() && self.current().lexeme == "require" {
                    if let Some(req) = self.try_parse_require() {
                        requires.push(req);
                        continue;
                    }
                }
            }

            match self.current().kind {
                TokenKind::LParen => depth_paren += 1,
                TokenKind::RParen => depth_paren = depth_paren.saturating_sub(1),
                TokenKind::LBracket => depth_bracket += 1,
                TokenKind::RBracket => depth_bracket = depth_bracket.saturating_sub(1),
                TokenKind::LBrace => depth_brace += 1,
                TokenKind::RBrace => {
                    if depth_brace == 0 {
                        break;
                    }
                    depth_brace -= 1;
                }
                TokenKind::String if depth_brace > 0 => {
                    self.validate_map_string_key();
                }
                _ => {}
            }
            self.advance();
        }
        requires
    }

    fn consume_expression_fragment(&mut self) {
        let _ = self.consume_expression_with_requires();
    }

    fn try_parse_require(&mut self) -> Option<RequireCall> {
        let start = self.current_span();
        if self.current().lexeme != "require" {
            return None;
        }
        self.advance();
        if !self.current_is(TokenKind::LParen) {
            return None;
        }
        self.advance();
        let path = if self.current().kind == TokenKind::String {
            let raw = self.current().lexeme.clone();
            let path = unquote_string(&raw);
            self.advance();
            path
        } else {
            self.error("string path expected in require()", self.current_span());
            String::new()
        };
        let _ = self.match_token(TokenKind::RParen);
        Some(RequireCall {
            path,
            span: start.merge(self.prev_span()),
        })
    }

    #[allow(dead_code)]
    fn putback(&mut self) {
        if self.index > 0 {
            self.index -= 1;
        }
    }

    fn skip_anonymous_function(&mut self) {
        if self.current().is_keyword(Keyword::Async) {
            self.advance();
        }
        if self.current().is_keyword(Keyword::Fun) {
            self.advance();
            if self.current().is_ident() {
                self.advance();
            }
            let _ = self.parse_param_list();
            if self.current_is(TokenKind::LBrace) {
                self.advance();
                self.skip_block_contents();
                let _ = self.match_token(TokenKind::RBrace);
            }
        }
    }

    fn validate_map_string_key(&mut self) {
        // map literal: after string key expect ':' or '='
        let next_index = self.index + 1;
        if next_index < self.tokens.len() {
            let next = &self.tokens[next_index].kind;
            if !matches!(next, TokenKind::Colon | TokenKind::Assign) {
                self.error(
                    "map key-value error: colon or assign expected",
                    self.tokens[next_index].span,
                );
            }
        }
    }

    fn skip_block_contents(&mut self) {
        let mut depth = 0i32;
        while !self.at_end() {
            match self.current().kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            self.advance();
        }
    }

    fn skip_balanced_brace(&mut self) {
        if !self.match_token(TokenKind::LBrace) {
            return;
        }
        self.skip_block_contents();
        let _ = self.match_token(TokenKind::RBrace);
    }

    fn skip_toplevel_brace_fragments(&mut self) {
        while self.current_is(TokenKind::LBrace) {
            self.skip_balanced_brace();
        }
    }

    fn skip_statement(&mut self) {
        if self.current_is(TokenKind::LBrace) {
            self.skip_balanced_brace();
            return;
        }
        self.consume_expression_fragment();
        let _ = self.match_token(TokenKind::Semi);
    }

    fn synchronize_toplevel(&mut self) {
        self.advance();
        while !self.at_end() {
            match &self.current().kind {
                TokenKind::Keyword(Keyword::Var)
                | TokenKind::Keyword(Keyword::Fun)
                | TokenKind::Keyword(Keyword::Class) => break,
                TokenKind::Semi => {
                    self.advance();
                    break;
                }
                _ => self.advance(),
            }
        }
    }

    fn synchronize_block(&mut self) {
        self.advance();
        while !self.at_end() && !self.current_is(TokenKind::RBrace) {
            match &self.current().kind {
                TokenKind::Keyword(Keyword::Var)
                | TokenKind::Keyword(Keyword::Fun)
                | TokenKind::Keyword(Keyword::Class)
                | TokenKind::Keyword(Keyword::Prop)
                | TokenKind::Keyword(Keyword::Return)
                | TokenKind::Keyword(Keyword::If)
                | TokenKind::Keyword(Keyword::Break) => break,
                TokenKind::Semi => {
                    self.advance();
                    break;
                }
                _ => self.advance(),
            }
        }
    }

    fn expect_semi(&mut self, context: &str) {
        if !self.match_token(TokenKind::Semi) {
            self.error(&format!("semicolon expected after {context}"), self.current_span());
        }
    }

    fn expect_lbrace(&mut self, context: &str) {
        if !self.match_token(TokenKind::LBrace) {
            self.error(&format!("'{{' expected for {context}"), self.current_span());
        }
    }

    fn expect_rbrace(&mut self, context: &str) {
        if !self.match_token(TokenKind::RBrace) {
            self.error(&format!("'}}' expected for {context}"), self.current_span());
        }
    }

    fn expect_ident(&mut self, context: &str) -> (String, Span) {
        if self.current().is_ident() {
            let name = self.current().lexeme.clone();
            let span = self.current_span();
            self.advance();
            (name, span)
        } else {
            self.error(&format!("identifier expected for {context}"), self.current_span());
            (String::new(), self.current_span())
        }
    }

    fn match_token(&mut self, kind: TokenKind) -> bool {
        if self.current().kind == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn current_is(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn current(&self) -> &Token {
        &self.tokens[self.index.min(self.tokens.len().saturating_sub(1))]
    }

    fn current_span(&self) -> Span {
        self.current().span
    }

    fn prev_span(&self) -> Span {
        if self.index == 0 {
            self.current().span
        } else {
            self.tokens[self.index - 1].span
        }
    }

    fn advance(&mut self) {
        if !self.at_end() {
            self.index += 1;
        }
    }

    fn at_end(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn error(&mut self, message: &str, span: Span) {
        self.errors.push(ParseError {
            message: message.to_string(),
            span,
        });
    }
}

fn unquote_string(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        raw[1..raw.len() - 1].to_string()
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sample_class_and_require() {
        let src = r#"require("./util.boyia");

class Printer {
    fun say(msg) {
        Util.log(msg);
    }
};
"#;
        let result = parse(src);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.program.items.len(), 2);
    }

    #[test]
    fn detect_missing_semicolon() {
        let src = "var x = 1\nclass A {}";
        let result = parse(src);
        assert!(result.errors.iter().any(|e| e.message.contains("semicolon")));
    }
}
