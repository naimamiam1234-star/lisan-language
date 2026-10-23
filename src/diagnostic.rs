use crate::source::{SourceMap, Span};
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub primary: Span,
    pub labels: Vec<(Span, String)>,
}
impl Diagnostic {
    pub fn new(code: &'static str, primary: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            primary,
            labels: vec![],
        }
    }
    pub fn render(&self, sources: &SourceMap) -> String {
        let (line, col) = sources.line_column(self.primary);
        let path = sources
            .sources
            .get(self.primary.source.0 as usize)
            .map(|s| s.path.display().to_string())
            .unwrap_or_else(|| "<source>".into());
        let mut result = format!("{path}:{line}:{col}: {}: {}", self.code, self.message);
        for (span, msg) in &self.labels {
            let (l, c) = sources.line_column(*span);
            let p = sources
                .sources
                .get(span.source.0 as usize)
                .map(|s| s.path.display().to_string())
                .unwrap_or_else(|| "<source>".into());
            result.push_str(&format!("\n  {p}:{l}:{c}: {msg}"));
        }
        result
    }
}
impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} (source {}, bytes {}..{})",
            self.code, self.message, self.primary.source.0, self.primary.start, self.primary.end
        )
    }
}
impl std::error::Error for Diagnostic {}
