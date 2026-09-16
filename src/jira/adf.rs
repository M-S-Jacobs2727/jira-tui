use serde_json::{Value, json};

pub fn text_to_adf(text: &str) -> Value {
    let paragraphs: Vec<Value> = if text.trim().is_empty() {
        vec![paragraph("")]
    } else {
        text.split("\n\n")
            .map(|block| {
                let lines: Vec<&str> = block.split('\n').collect();
                let mut content = Vec::new();
                for (i, line) in lines.iter().enumerate() {
                    if !line.is_empty() {
                        content.push(json!({ "type": "text", "text": line }));
                    }
                    if i + 1 < lines.len() {
                        content.push(json!({ "type": "hardBreak" }));
                    }
                }
                if content.is_empty() {
                    paragraph("")
                } else {
                    json!({ "type": "paragraph", "content": content })
                }
            })
            .collect()
    };

    json!({
        "type": "doc",
        "version": 1,
        "content": paragraphs,
    })
}

pub fn adf_to_text(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    let mut out = String::new();
    walk(value, &mut out);
    out.trim_end().to_string()
}

fn paragraph(text: &str) -> Value {
    if text.is_empty() {
        json!({ "type": "paragraph", "content": [] })
    } else {
        json!({
            "type": "paragraph",
            "content": [{ "type": "text", "text": text }]
        })
    }
}

fn walk(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let ty = map.get("type").and_then(Value::as_str).unwrap_or("");
            match ty {
                "text" => {
                    if let Some(text) = map.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                    }
                }
                "hardBreak" => out.push('\n'),
                "paragraph" | "heading" => {
                    if let Some(content) = map.get("content") {
                        walk(content, out);
                    }
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                "bulletList" | "orderedList" | "listItem" | "blockquote" | "panel" | "doc" => {
                    if let Some(content) = map.get("content") {
                        walk(content, out);
                    }
                    if ty == "listItem" && !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                "mention" => {
                    if let Some(text) = map.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                    }
                }
                "emoji" => {
                    if let Some(text) = map
                        .get("attrs")
                        .and_then(|a| a.get("shortName"))
                        .and_then(Value::as_str)
                    {
                        out.push_str(text);
                    }
                }
                _ => {
                    if let Some(content) = map.get("content") {
                        walk(content, out);
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_plain_text() {
        let original = "Hello world\n\nSecond paragraph";
        let adf = text_to_adf(original);
        assert_eq!(adf["type"], "doc");
        assert_eq!(adf["version"], 1);
        let back = adf_to_text(&adf);
        assert!(back.contains("Hello world"));
        assert!(back.contains("Second paragraph"));
    }

    #[test]
    fn empty_text_is_valid_doc() {
        let adf = text_to_adf("");
        assert_eq!(adf["type"], "doc");
        assert!(adf["content"].as_array().is_some());
    }

    #[test]
    fn walks_nested_lists() {
        let adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "bulletList",
                "content": [{
                    "type": "listItem",
                    "content": [{
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "item" }]
                    }]
                }]
            }]
        });
        assert_eq!(adf_to_text(&adf), "item");
    }
}
