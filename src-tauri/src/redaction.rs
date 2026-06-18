pub fn redact_diagnostic(input: &str) -> String {
    let normalized = redact_assignment_values(input);
    let normalized = redact_inline_secrets(&normalized);
    normalized
        .split_whitespace()
        .map(redact_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_assignment_values(input: &str) -> String {
    let mut redacted = input.to_string();
    for key in ["OPENAI_API_KEY", "api_key", "token", "account_id"] {
        redacted = redact_json_string_value(&redacted, key);
        redacted = redact_env_value(&redacted, key);
    }
    redacted
}

fn redact_env_value(input: &str, key: &str) -> String {
    let marker = format!("{key}=");
    let mut output = String::new();
    let mut rest = input;
    while let Some(index) = rest.find(&marker) {
        output.push_str(&rest[..index]);
        output.push_str(&marker);
        output.push_str("<redacted>");
        let value_start = index + marker.len();
        let value_end = rest[value_start..]
            .find(char::is_whitespace)
            .map(|offset| value_start + offset)
            .unwrap_or(rest.len());
        rest = &rest[value_end..];
    }
    output.push_str(rest);
    output
}

fn redact_json_string_value(input: &str, key: &str) -> String {
    let marker = format!("\"{key}\"");
    let mut output = String::new();
    let mut rest = input;
    while let Some(index) = rest.find(&marker) {
        output.push_str(&rest[..index]);
        output.push_str(&marker);
        let after_key = index + marker.len();
        let Some((prefix_end, value_start)) = find_json_string_value_start(rest, after_key) else {
            rest = &rest[after_key..];
            continue;
        };
        output.push_str(&rest[after_key..prefix_end]);
        output.push('"');
        output.push_str("<redacted>");
        let value_end = rest[value_start..]
            .find('"')
            .map(|offset| value_start + offset)
            .unwrap_or(rest.len());
        rest = &rest[value_end..];
    }
    output.push_str(rest);
    output
}

fn find_json_string_value_start(input: &str, after_key: usize) -> Option<(usize, usize)> {
    let mut cursor = after_key;
    cursor += input[cursor..]
        .chars()
        .take_while(|character| character.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    if input[cursor..].chars().next()? != ':' {
        return None;
    }
    cursor += 1;
    cursor += input[cursor..]
        .chars()
        .take_while(|character| character.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    if input[cursor..].chars().next()? != '"' {
        return None;
    }
    Some((cursor, cursor + 1))
}

fn redact_inline_secrets(input: &str) -> String {
    let mut output = String::new();
    let mut index = 0;
    while let Some(relative_start) = input[index..].find("sk-") {
        let start = index + relative_start;
        output.push_str(&input[index..start]);
        output.push_str("sk-<redacted>");
        let mut end = start;
        for (offset, character) in input[start..].char_indices() {
            if offset == 0 {
                continue;
            }
            if !(character.is_ascii_alphanumeric() || character == '-' || character == '_') {
                break;
            }
            end = start + offset + character.len_utf8();
        }
        if end == start {
            end = start + 3;
        }
        index = end;
    }
    output.push_str(&input[index..]);
    output
}

fn redact_token(token: &str) -> String {
    let mut redacted = redact_windows_user_path(token);
    redacted = redact_secret_like_token(&redacted);
    redact_email(&redacted)
}

fn redact_windows_user_path(token: &str) -> String {
    let normalized = token.replace('/', "\\");
    let marker = "C:\\Users\\";
    let Some(start) = normalized.find(marker) else {
        return token.to_string();
    };
    let rest = &normalized[start + marker.len()..];
    let Some(end) = rest.find('\\') else {
        return format!("{}{}", &normalized[..start], "C:\\Users\\<user>");
    };
    format!(
        "{}{}{}",
        &normalized[..start],
        "C:\\Users\\<user>",
        &rest[end..]
    )
}

fn redact_secret_like_token(token: &str) -> String {
    if token.starts_with("sk-") || token.starts_with("sk_") {
        return "sk-<redacted>".to_string();
    }
    token.to_string()
}

fn redact_email(token: &str) -> String {
    if token.contains('@') && token.contains('.') {
        "<email>".to_string()
    } else {
        token.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::redaction::redact_diagnostic;

    #[test]
    fn redacts_windows_user_paths_and_tokens() {
        let redacted = redact_diagnostic(
            r#"C:\Users\Ruping.Liu\.codex\auth.json token sk-proj-secret email user@example.com"#,
        );

        assert!(!redacted.contains("Ruping.Liu"));
        assert!(!redacted.contains("sk-proj-secret"));
        assert!(!redacted.contains("user@example.com"));
        assert!(redacted.contains("C:\\Users\\<user>"));
        assert!(redacted.contains("sk-<redacted>"));
    }

    #[test]
    fn redacts_embedded_env_json_tokens_and_account_ids() {
        let redacted = redact_diagnostic(
            r#"OPENAI_API_KEY=sk-proj-secret {"token":"sk-proj-json","account_id":"acct_123456"}"#,
        );

        assert!(!redacted.contains("sk-proj-secret"));
        assert!(!redacted.contains("sk-proj-json"));
        assert!(!redacted.contains("acct_123456"));
        assert!(redacted.contains("OPENAI_API_KEY=<redacted>"));
        assert!(redacted.contains("\"token\":\"<redacted>\""));
        assert!(redacted.contains("\"account_id\":\"<redacted>\""));
    }

    #[test]
    fn redacts_json_token_values_with_whitespace() {
        let redacted = redact_diagnostic(r#"{"token": "non_sk_secret", "account_id": "acct_789"}"#);

        assert!(!redacted.contains("non_sk_secret"));
        assert!(!redacted.contains("acct_789"));
        assert!(redacted.contains("\"token\": \"<redacted>\""));
        assert!(redacted.contains("\"account_id\": \"<redacted>\""));
    }
}
