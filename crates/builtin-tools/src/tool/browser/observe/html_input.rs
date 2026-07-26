use serde_json::{json, Value};

use super::super::types::BrowserHtmlInputReceipt;

pub const DEFAULT_INLINE_HTML_LIMIT: usize = 24_000;

pub fn html_input_receipt(html: &str, inline_chars: usize) -> BrowserHtmlInputReceipt {
    BrowserHtmlInputReceipt::public_snapshot(html.chars().count(), inline_chars)
}

pub fn html_input_receipt_payload(html: &str, inline_chars: usize) -> Value {
    json!(html_input_receipt(html, inline_chars))
}
