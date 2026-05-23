//! Syntax diagnostics from the local Boyia parser.

use crate::document::{analyze_source, diagnostics_from_analysis};
use tower_lsp::lsp_types::{Diagnostic, Url};

pub fn compile_diagnostics(text: &str, _uri: &Url) -> Vec<Diagnostic> {
    let analysis = analyze_source(text);
    diagnostics_from_analysis(text, &analysis)
}
