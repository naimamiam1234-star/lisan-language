//! UTF-8 source offsets with human-readable Unicode-scalar columns.
use std::path::PathBuf;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SourceId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}
impl Span {
    pub fn join(self, other: Self) -> Self {
        Self {
            end: other.end,
            ..self
        }
    }
}
#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
    pub text: String,
}
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    pub sources: Vec<Source>,
}
impl SourceMap {
    pub fn add(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> SourceId {
        let id = SourceId(self.sources.len() as u32);
        self.sources.push(Source {
            path: path.into(),
            text: text.into(),
        });
        id
    }
    pub fn text(&self, id: SourceId) -> Option<&str> {
        self.sources.get(id.0 as usize).map(|s| s.text.as_str())
    }
    pub fn line_column(&self, span: Span) -> (usize, usize) {
        self.text(span.source)
            .map(|s| {
                let mut pos = span.start.min(s.len());
                while !s.is_char_boundary(pos) {
                    pos -= 1;
                }
                let before = &s[..pos];
                (
                    before.chars().filter(|c| *c == '\n').count() + 1,
                    before.rsplit('\n').next().unwrap_or("").chars().count() + 1,
                )
            })
            .unwrap_or((1, 1))
    }
}
