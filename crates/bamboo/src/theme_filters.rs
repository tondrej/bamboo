//! Custom Tera filters registered by [`ThemeEngine`](crate::ThemeEngine).

use crate::parsing::slugify;
use std::collections::HashMap;
use tera::Tera;

pub(crate) fn register_custom_filters(tera: &mut Tera) {
    tera.register_filter(
        "reading_time",
        |value: &tera::Value, _args: &HashMap<String, tera::Value>| {
            let raw_text = value.as_str().unwrap_or("");
            let plain_text = crate::search::strip_html_tags(raw_text);
            let words = crate::parsing::word_count(&plain_text);
            let result = crate::parsing::reading_time(words);
            Ok(tera::Value::Number(serde_json::Number::from(result)))
        },
    );

    tera.register_filter(
        "word_count",
        |value: &tera::Value, _args: &HashMap<String, tera::Value>| {
            let raw_text = value.as_str().unwrap_or("");
            let plain_text = crate::search::strip_html_tags(raw_text);
            let count = crate::parsing::word_count(&plain_text);
            Ok(tera::Value::Number(serde_json::Number::from(count)))
        },
    );

    tera.register_filter(
        "toc",
        |value: &tera::Value, _args: &HashMap<String, tera::Value>| {
            let empty = Vec::new();
            let entries = value.as_array().unwrap_or(&empty);
            let mut html = String::from("<ul class=\"toc\">\n");
            for entry in entries {
                let level = entry.get("level").and_then(|v| v.as_u64()).unwrap_or(1);
                let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let title = entry.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let indent = "  ".repeat(level as usize);
                let escaped_title = crate::xml::escape(title);
                let escaped_id = crate::xml::escape(id);
                html.push_str(&format!(
                    "{indent}<li class=\"toc-level-{level}\"><a href=\"#{escaped_id}\">{escaped_title}</a></li>\n"
                ));
            }
            html.push_str("</ul>");
            Ok(tera::Value::String(html))
        },
    );

    tera.register_filter(
        "slugify",
        |value: &tera::Value, _args: &HashMap<String, tera::Value>| {
            let text = value.as_str().unwrap_or("");
            Ok(tera::Value::String(slugify(text)))
        },
    );
}
