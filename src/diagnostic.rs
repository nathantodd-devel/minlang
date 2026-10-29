//! Renders errors with the source line they point at, in the style of rustc:
//!
//! ```text
//! error: Undefined name 'count'
//!  --> demo.pk:2:13
//!   |
//! 2 |     let a = count + 1;
//!   |             ^^^^^
//! ```

use std::fmt::{self, Write};

use crate::error::GenericError;

const RED: &str = "\x1b[1;31m";
const BLUE: &str = "\x1b[1;34m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

pub struct Renderer<'a> {
    lines: Vec<&'a str>,
    color: bool,
}

impl<'a> Renderer<'a> {
    pub fn new(source: &'a str, color: bool) -> Self {
        let lines = source
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect();
        Self { lines, color }
    }

    fn paint(&self, style: &str, text: &str) -> String {
        if self.color {
            format!("{style}{text}{RESET}")
        } else {
            text.to_owned()
        }
    }

    /// The `error: message` header that starts every diagnostic.
    pub fn header(&self, message: &str) -> String {
        format!(
            "{}{}",
            self.paint(RED, "error"),
            self.paint(BOLD, &format!(": {message}"))
        )
    }

    pub fn render<K: fmt::Display>(&self, error: &GenericError<K>) -> String {
        let line_number = (error.line + 1).to_string();
        let gutter = " ".repeat(line_number.len());
        let bar = self.paint(BLUE, "|");

        let mut text = self.header(&error.message());
        let _ = write!(
            text,
            "\n{gutter}{} {}:{}:{}",
            self.paint(BLUE, "-->"),
            error.file.as_deref().unwrap_or("stdin"),
            error.line + 1,
            error.column + 1
        );

        let Some(source) = self.lines.get(error.line) else {
            return text;
        };

        // Tabs are shown as four spaces, so the caret must account for them
        let chars: Vec<char> = source.chars().collect();
        let shown: String = source.replace('\t', "    ");
        let offset: usize = chars
            .iter()
            .take(error.column)
            .map(|&c| if c == '\t' { 4 } else { 1 })
            .sum();
        let underline = "^".repeat(token_length(&chars, error.column));

        let _ = write!(
            text,
            "\n{gutter} {bar}\n{} {bar} {shown}\n{gutter} {bar} {}{}",
            self.paint(BLUE, &line_number),
            " ".repeat(offset),
            self.paint(RED, &underline)
        );
        text
    }
}

/// The length of the token starting at `column`: a whole name or number, or
/// one character otherwise.
fn token_length(chars: &[char], column: usize) -> usize {
    let word = |c: &char| c.is_ascii_alphanumeric() || *c == '_';
    match chars.get(column) {
        Some(c) if word(c) => chars[column..].iter().take_while(|c| word(c)).count(),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(line: usize, column: usize) -> GenericError<String> {
        GenericError::new(
            "Undefined name 'count'".to_owned(),
            line,
            column,
            Some("demo.pk"),
        )
    }

    #[test]
    fn renders_the_source_line() {
        let source = "fn f() {\n    let a = count + 1;\n}";
        assert_eq!(
            Renderer::new(source, false).render(&error(1, 12)),
            "error: Undefined name 'count'\n \
             --> demo.pk:2:13\n  \
             |\n\
             2 |     let a = count + 1;\n  \
             |             ^^^^^"
        );
    }

    #[test]
    fn underlines_one_character_for_symbols() {
        let rendered = Renderer::new("let a = ;", false).render(&error(0, 8));
        assert!(rendered.ends_with("|         ^"), "{rendered}");
    }

    #[test]
    fn widens_the_gutter_for_long_line_numbers() {
        let source = "\n".repeat(11) + "x";
        let rendered = Renderer::new(&source, false).render(&error(11, 0));
        assert!(rendered.contains("\n  --> demo.pk:12:1"), "{rendered}");
        assert!(rendered.contains("\n12 | x\n   | ^"), "{rendered}");
    }

    #[test]
    fn accounts_for_tabs() {
        let rendered = Renderer::new("\tx = y;", false).render(&error(0, 5));
        assert!(rendered.ends_with("|         ^"), "{rendered}");
    }

    #[test]
    fn colors_when_asked() {
        let rendered = Renderer::new("x", true).render(&error(0, 0));
        assert!(rendered.starts_with("\x1b[1;31merror\x1b[0m"));
    }

    #[test]
    fn positions_past_the_end_have_no_snippet() {
        let rendered = Renderer::new("x", false).render(&error(5, 0));
        assert_eq!(rendered, "error: Undefined name 'count'\n --> demo.pk:6:1");
    }
}
