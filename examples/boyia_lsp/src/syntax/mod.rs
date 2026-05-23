mod analyze;
mod ast;
mod lexer;
mod parser;
mod span;
mod token;

pub use analyze::{
    analyze, completion_labels, diagnostics_from_analysis, find_definition_at, find_hover_at,
    location_in_file, require_at_position, to_document_symbols, Analysis,
};
pub use parser::ParseResult;
pub use span::word_at_position;
pub use token::BOYIA_KEYWORDS;

pub fn parse(source: &str) -> ParseResult {
    parser::parse(source)
}
