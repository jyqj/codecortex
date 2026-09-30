//! JSONC comments/trailing commas only; strings remain byte-for-byte intact.
use cc_model::{CcError, CcResult};
pub(super) fn parse(text: &str) -> CcResult<serde_json::Value> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut bytes = text.as_bytes().to_vec();
    let mut i = 0;
    let mut string = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                string = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            string = true;
            i += 1;
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                bytes[i] = b' ';
                i += 1;
            }
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    bytes[i] = b' ';
                    bytes[i + 1] = b' ';
                    i += 2;
                    closed = true;
                    break;
                }
                if bytes[i] != b'\n' && bytes[i] != b'\r' {
                    bytes[i] = b' ';
                }
                i += 1;
            }
            if !closed {
                return Err(CcError::Config("unterminated JSONC comment".into()));
            }
            continue;
        }
        i += 1;
    }
    i = 0;
    string = false;
    escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                string = false;
            }
        } else if b == b'"' {
            string = true;
        } else if b == b',' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if matches!(bytes.get(j), Some(b'}' | b']')) {
                bytes[i] = b' ';
            }
        }
        i += 1;
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        CcError::Config(format!(
            "invalid JSONC at line {} column {}",
            e.line(),
            e.column()
        ))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_do_not_modify_urls_escapes_or_unicode() {
        let v=parse(r#"{ /*c*/ "url":"https://example/*not a comment*/", "quote":"\\\"//", "中文":"值", "list":[1,2,], }"#).unwrap();
        assert_eq!(v["url"], "https://example/*not a comment*/");
        assert_eq!(v["中文"], "值");
        assert_eq!(v["list"], serde_json::json!([1, 2]));
    }
    #[test]
    fn bad_or_extended_json_is_rejected() {
        for text in ["{/*", "{x:1}", "{'x':1}", "{\"x\":\"bad}", "{\"x\":1,,}"] {
            assert!(parse(text).is_err(), "{text}");
        }
    }
}
