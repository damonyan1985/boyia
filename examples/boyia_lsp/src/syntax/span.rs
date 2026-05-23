use tower_lsp::lsp_types::{Position, Range};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

pub fn position_at_offset(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let character = (offset - line_start) as u32;
    Position { line, character }
}

pub fn offset_at_position(text: &str, position: Position) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    if position.line as usize >= lines.len() {
        return text.len();
    }
    let line_start = text
        .lines()
        .take(position.line as usize)
        .map(|l| l.len() + 1)
        .sum::<usize>();
    let line = lines[position.line as usize];
    let col = (position.character as usize).min(line.len());
    line_start + col
}

pub fn range_from_span(text: &str, span: Span) -> Range {
    Range {
        start: position_at_offset(text, span.start),
        end: position_at_offset(text, span.end),
    }
}

pub fn word_at_position(text: &str, position: Position) -> Option<String> {
    let offset = offset_at_position(text, position);
    let bytes = text.as_bytes();
    if offset > bytes.len() {
        return None;
    }
    let mut start = offset;
    while start > 0 && is_ident_part(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_ident_part(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(text[start..end].to_string())
}

fn is_ident_part(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}
