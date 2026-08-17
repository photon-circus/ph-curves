//! Markdown-safe formatting for TOML-derived generated documentation.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::format;
use std::prelude::v1::*;

/// Debug-format one string, then neutralize Markdown and HTML punctuation.
pub(crate) fn markdown_debug(value: &str) -> String {
    let debug = format!("{value:?}");
    let mut escaped = String::with_capacity(debug.len());
    for character in debug.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '`' | '*' | '_' | '{' | '}' | '[' | ']' | '<' | '>' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

/// Debug-format user text for generated rustdoc without creating Markdown.
///
/// Ordinary punctuation uses backslash/HTML escaping to keep the historical
/// output stable. URLs and literal backticks use an inline-code fence longer
/// than every backtick run in the debug string; this also satisfies
/// `rustdoc::bare_urls` when warnings are denied.
pub(crate) fn rustdoc_debug(value: &str) -> String {
    if value.contains("://") || value.contains('`') {
        let debug = format!("{value:?}");
        let mut longest_run = 0;
        let mut current_run = 0;
        for character in debug.chars() {
            if character == '`' {
                current_run += 1;
                longest_run = longest_run.max(current_run);
            } else {
                current_run = 0;
            }
        }
        let fence = "`".repeat(longest_run + 1);
        format!("{fence}{debug}{fence}")
    } else {
        markdown_debug(value)
    }
}
