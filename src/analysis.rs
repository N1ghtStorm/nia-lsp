use nialang::parser::{Parser, tokenize};
use nialang::semantics::typecheck::{check_fn, collect_sigs};
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, Position, Range};

pub fn diagnostics(source: &str) -> Vec<Diagnostic> {
    if let Some(diagnostic) = lexical_error(source) {
        return vec![diagnostic];
    }
    let (structs, enums, functions, vectors) = match Parser::new(tokenize(source)).parse_file() {
        Ok(file) => file,
        Err(message) => return vec![error(format!("parse error: {message}"))],
    };
    let (structs, enums, vectors, signatures) =
        match collect_sigs(&structs, &enums, &vectors, &functions) {
            Ok(symbols) => symbols,
            Err(message) => return vec![error(format!("semantic error: {message}"))],
        };

    functions
        .iter()
        .filter_map(|function| {
            check_fn(function, &structs, &enums, &vectors, &signatures)
                .err()
                .map(|message| error(format!("type error in `{}`: {message}", function.name)))
        })
        .collect()
}

fn error(message: String) -> Diagnostic {
    Diagnostic {
        // The compiler's parser and type checker do not expose source spans yet.
        range: Range::default(),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("nia".into()),
        message,
        ..Default::default()
    }
}

/// The compiler lexer currently treats unsupported characters as EOF. Reject
/// them before parsing so a partially tokenized file cannot appear valid.
fn lexical_error(source: &str) -> Option<Diagnostic> {
    let mut chars = source.chars().peekable();
    let mut position = Position::default();
    let mut string_start = None;
    let mut escaped = false;
    let mut comment = false;

    while let Some(ch) = chars.next() {
        let start = position;
        if ch == '\n' {
            position.line += 1;
            position.character = 0;
        } else {
            position.character += ch.len_utf16() as u32;
        }

        if comment {
            comment = ch != '\n';
        } else if string_start.is_some() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                string_start = None;
            }
        } else if ch == '/' && chars.peek() == Some(&'/') {
            comment = true;
        } else if ch == '"' {
            string_start = Some(start);
        } else if !ch.is_whitespace()
            && !ch.is_ascii_alphanumeric()
            && !"_:,;(){}[]+-*@/%&|^~.=!<>".contains(ch)
        {
            let mut diagnostic = error(format!("lex error: unsupported character `{ch}`"));
            diagnostic.range = Range::new(start, position);
            return Some(diagnostic);
        }
    }

    string_start.map(|start| {
        let mut diagnostic = error("lex error: unterminated string literal".into());
        diagnostic.range = Range::new(start, position);
        diagnostic
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_source_and_inline_modules() {
        assert!(diagnostics("").is_empty());
        assert!(
            diagnostics("mod math { fn answer() i32 { 42 } } fn main() i32 { math::answer() }")
                .is_empty()
        );
        assert!(diagnostics("fn main() i32 { println(\"Привет 🌍\"); 0 }").is_empty());
    }

    #[test]
    fn reports_parser_signature_and_type_errors() {
        assert!(
            diagnostics("fn main( { 0 }")[0]
                .message
                .starts_with("parse error:")
        );
        assert!(
            diagnostics("fn a() {} fn a() {}")[0]
                .message
                .contains("duplicate")
        );
        let errors = diagnostics("fn a() i32 { true } fn b() bool { 42 }");
        assert_eq!(errors.len(), 2);
        assert!(errors[0].message.starts_with("type error in `a`:"));
        assert!(errors[1].message.starts_with("type error in `b`:"));
    }

    #[test]
    fn rejects_characters_that_would_truncate_the_token_stream() {
        let errors = diagnostics("fn main() i32 { 0 }\n💥");
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].range,
            Range::new(Position::new(1, 0), Position::new(1, 2))
        );
        assert!(errors[0].message.starts_with("lex error:"));
    }

    #[test]
    fn lexical_validation_handles_strings_comments_and_utf16() {
        assert!(lexical_error("// 💥\r\n\"🌍\\\"//\n💥\"").is_none());
        let diagnostic = lexical_error("\"🌍\" 💥").unwrap();
        assert_eq!(
            diagnostic.range,
            Range::new(Position::new(0, 5), Position::new(0, 7))
        );
        assert!(
            diagnostics("fn main() { println(\"oops")[0]
                .message
                .contains("unterminated")
        );
    }
}
