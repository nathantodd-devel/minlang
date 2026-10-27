use std::fmt;

/// A positioned error, shared by every compiler stage. `K` describes what went
/// wrong; the rest says where.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericError<K> {
    pub kind: K,
    pub line: usize,
    pub column: usize,
    pub file: Option<String>,
}

impl<K: fmt::Display> GenericError<K> {
    pub fn new(kind: K, line: usize, column: usize, file: Option<&str>) -> Self {
        Self {
            kind,
            line,
            column,
            file: file.map(str::to_owned),
        }
    }

    pub fn message(&self) -> String {
        self.kind.to_string()
    }

    pub fn format(&self) -> String {
        format!(
            "{}:{}:{}: {}",
            self.file.as_deref().unwrap_or("stdin"),
            self.line,
            self.column,
            self.kind
        )
    }
}

impl<K: fmt::Display> fmt::Display for GenericError<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

impl<K: fmt::Display + fmt::Debug> std::error::Error for GenericError<K> {}
