//! Document helpers built on the Boyia syntax frontend.

use crate::syntax::{
    analyze, completion_labels, find_definition_at, find_hover_at, location_in_file,
    require_at_position, to_document_symbols, word_at_position, Analysis,
};
use tower_lsp::lsp_types::{DocumentSymbol, Location, Position, Range, Url};

pub fn analyze_source(text: &str) -> Analysis {
    analyze(text)
}

pub fn document_symbols(text: &str, analysis: &Analysis) -> Vec<DocumentSymbol> {
    to_document_symbols(text, analysis)
}

pub fn completion_items(analysis: &Analysis) -> Vec<String> {
    completion_labels(analysis)
}

pub fn find_hover(text: &str, analysis: &Analysis, position: Position) -> Option<String> {
    find_hover_at(text, analysis, position)
}

pub fn find_definition(text: &str, analysis: &Analysis, position: Position) -> Option<Range> {
    find_definition_at(text, analysis, position)
}

pub fn require_target(text: &str, analysis: &Analysis, position: Position) -> Option<(Range, String)> {
    require_at_position(text, analysis, position)
}

pub fn word_at(text: &str, position: Position) -> Option<String> {
    word_at_position(text, position)
}

pub fn goto_location(uri: &Url, range: Range) -> Location {
    location_in_file(uri, range)
}

pub use crate::syntax::BOYIA_KEYWORDS as KEYWORDS;

pub use crate::syntax::diagnostics_from_analysis;
