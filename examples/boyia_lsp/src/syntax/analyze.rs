use super::ast::*;
use super::parser::{ParseError, ParseResult};
use super::span::{offset_at_position, range_from_span, Span, word_at_position};
use super::token::BOYIA_KEYWORDS;
use tower_lsp::lsp_types::{Location, Position, Range, SymbolKind, Url};

#[derive(Clone, Debug)]
pub struct SymbolDef {
    pub name: String,
    pub kind: SymbolKind,
    pub name_span: Span,
    pub range_span: Span,
    pub detail: Option<String>,
    pub container: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub symbols: Vec<SymbolDef>,
    pub requires: Vec<RequireCall>,
    pub errors: Vec<ParseError>,
}

pub fn analyze(source: &str) -> Analysis {
    let ParseResult { program, errors } = super::parse(source);
    let mut symbols = Vec::new();
    let mut requires = Vec::new();
    collect_program(source, &program, &mut symbols, &mut requires);
    Analysis {
        symbols,
        requires,
        errors,
    }
}

fn collect_program(
    _source: &str,
    program: &Program,
    symbols: &mut Vec<SymbolDef>,
    requires: &mut Vec<RequireCall>,
) {
    for item in &program.items {
        match item {
            TopLevelItem::VarDecl(v) => {
                for binding in &v.names {
                    symbols.push(SymbolDef {
                        name: binding.name.clone(),
                        kind: SymbolKind::VARIABLE,
                        name_span: binding.name_span,
                        range_span: v.span,
                        detail: Some("variable".into()),
                        container: None,
                    });
                }
            }
            TopLevelItem::FunDecl(f) => {
                symbols.push(fun_symbol(f, SymbolKind::FUNCTION, None));
                collect_block_locals(f, symbols);
            }
            TopLevelItem::ClassDecl(c) => {
                let detail = c
                    .extends
                    .as_ref()
                    .map(|(n, _)| format!("extends {n}"));
                symbols.push(SymbolDef {
                    name: c.name.clone(),
                    kind: SymbolKind::CLASS,
                    name_span: c.name_span,
                    range_span: c.span,
                    detail,
                    container: None,
                });
                for member in &c.members {
                    match member {
                        ClassMember::PropAssign { name, name_span, span } => {
                            symbols.push(SymbolDef {
                                name: name.clone(),
                                kind: SymbolKind::PROPERTY,
                                name_span: *name_span,
                                range_span: *span,
                                detail: Some("prop".into()),
                                container: Some(c.name.clone()),
                            });
                        }
                        ClassMember::Method(m) => {
                            symbols.push(fun_symbol(
                                m,
                                SymbolKind::METHOD,
                                Some(c.name.clone()),
                            ));
                            collect_block_locals(m, symbols);
                        }
                    }
                }
            }
            TopLevelItem::ExprStmt(e) => {
                requires.extend(e.requires.clone());
            }
        }
    }
}

fn fun_symbol(fun: &FunDecl, kind: SymbolKind, container: Option<String>) -> SymbolDef {
    let detail = match fun.kind {
        FunKind::Function => "function",
        FunKind::AsyncFunction => "async function",
        FunKind::Method => "method",
        FunKind::PropMethod => "prop method",
        FunKind::AsyncPropMethod => "prop async method",
    };
    SymbolDef {
        name: fun.name.clone(),
        kind,
        name_span: fun.name_span,
        range_span: fun.span,
        detail: Some(detail.into()),
        container,
    }
}

fn collect_block_locals(fun: &FunDecl, symbols: &mut Vec<SymbolDef>) {
    collect_block(&fun.body, &fun.name, symbols);
}

fn collect_block(block: &Block, container: &str, symbols: &mut Vec<SymbolDef>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::VarDecl(v) => {
                for binding in &v.names {
                    symbols.push(SymbolDef {
                        name: binding.name.clone(),
                        kind: SymbolKind::VARIABLE,
                        name_span: binding.name_span,
                        range_span: v.span,
                        detail: Some("local variable".into()),
                        container: Some(container.to_string()),
                    });
                }
            }
            Stmt::FunDecl(f) => {
                symbols.push(fun_symbol(f, SymbolKind::FUNCTION, Some(container.to_string())));
                collect_block_locals(f, symbols);
            }
            Stmt::Block(b) => collect_block(b, container, symbols),
            _ => {}
        }
    }
}

pub fn completion_labels(analysis: &Analysis) -> Vec<String> {
    let mut items: Vec<String> = BOYIA_KEYWORDS.iter().map(|s| s.to_string()).collect();
    for sym in &analysis.symbols {
        if !items.iter().any(|i| i == &sym.name) {
            items.push(sym.name.clone());
        }
    }
    items.sort();
    items
}

pub fn find_hover_at(source: &str, analysis: &Analysis, position: Position) -> Option<String> {
    let offset = offset_at_position(source, position);
    let word = word_at_position(source, position)?;
    if word.is_empty() {
        return None;
    }

    if let Some(sym) = symbol_at_offset(analysis, offset, &word) {
        let kind = symbol_kind_label(sym.kind);
        return Some(if let Some(detail) = &sym.detail {
            if let Some(container) = &sym.container {
                format!("**{}** — {} in `{container}` ({detail})", sym.name, kind)
            } else {
                format!("**{}** — {} ({detail})", sym.name, kind)
            }
        } else {
            format!("**{}** — {}", sym.name, kind)
        });
    }

    if BOYIA_KEYWORDS.contains(&word.as_str()) {
        return Some(format!("**{}** — keyword", word));
    }
    None
}

pub fn find_definition_at(
    source: &str,
    analysis: &Analysis,
    position: Position,
) -> Option<Range> {
    let offset = offset_at_position(source, position);
    let word = word_at_position(source, position)?;
    if word.is_empty() {
        return None;
    }
    symbol_at_offset(analysis, offset, &word).map(|s| range_from_span(source, s.name_span))
}

pub fn require_at_position(
    source: &str,
    analysis: &Analysis,
    position: Position,
) -> Option<(Range, String)> {
    let offset = offset_at_position(source, position);
    for req in &analysis.requires {
        if offset >= req.span.start && offset <= req.span.end {
            return Some((range_from_span(source, req.span), req.path.clone()));
        }
    }
    None
}

fn symbol_at_offset<'a>(
    analysis: &'a Analysis,
    offset: usize,
    word: &str,
) -> Option<&'a SymbolDef> {
  analysis
        .symbols
        .iter()
        .filter(|s| s.name == word && offset >= s.name_span.start && offset <= s.name_span.end)
        .max_by_key(|s| s.name_span.start)
}

fn symbol_kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::CLASS => "class",
        SymbolKind::FUNCTION => "function",
        SymbolKind::METHOD => "method",
        SymbolKind::VARIABLE => "variable",
        SymbolKind::PROPERTY => "property",
        _ => "symbol",
    }
}

pub fn to_document_symbols(source: &str, analysis: &Analysis) -> Vec<tower_lsp::lsp_types::DocumentSymbol> {
    use tower_lsp::lsp_types::DocumentSymbol;

    let mut top_level: Vec<DocumentSymbol> = Vec::new();
    let mut class_children: std::collections::HashMap<String, Vec<DocumentSymbol>> =
        std::collections::HashMap::new();

    for sym in &analysis.symbols {
        if let Some(container) = &sym.container {
            class_children
                .entry(container.clone())
                .or_default()
                .push(to_doc_symbol(source, sym));
        } else if !(sym.kind == SymbolKind::VARIABLE && sym.container.is_some()) {
            top_level.push(to_doc_symbol(source, sym));
        }
    }

    for doc_sym in &mut top_level {
        if doc_sym.kind == SymbolKind::CLASS {
            if let Some(children) = class_children.remove(&doc_sym.name) {
                doc_sym.children = Some(children);
            }
        }
    }

    top_level
}

fn to_doc_symbol(source: &str, sym: &SymbolDef) -> tower_lsp::lsp_types::DocumentSymbol {
    use tower_lsp::lsp_types::DocumentSymbol;
    DocumentSymbol {
        name: sym.name.clone(),
        detail: sym.detail.clone(),
        kind: sym.kind,
        tags: None,
        #[allow(deprecated)]
        deprecated: None,
        range: range_from_span(source, sym.range_span),
        selection_range: range_from_span(source, sym.name_span),
        children: None,
    }
}

pub fn location_in_file(uri: &Url, range: Range) -> Location {
    Location {
        uri: uri.clone(),
        range,
    }
}

pub fn diagnostics_from_analysis(
    source: &str,
    analysis: &Analysis,
) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

    analysis
        .errors
        .iter()
        .map(|err| {
            let range = range_from_span(source, err.span);
            Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("boyia".into()),
                message: err.message.clone(),
                related_information: None,
                tags: None,
                data: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_include_class_and_method() {
        let src = "class A { fun m() {} }";
        let analysis = analyze(src);
        assert!(analysis.symbols.iter().any(|s| s.name == "A"));
        assert!(analysis.symbols.iter().any(|s| s.name == "m"));
    }
}
