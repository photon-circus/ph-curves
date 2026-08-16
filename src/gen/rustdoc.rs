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
