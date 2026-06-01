use regex::Regex;

pub fn redact(input: &str) -> String {
    let mut value = input.to_string();

    for (pattern, replacement) in patterns() {
        value = pattern.replace_all(&value, replacement).to_string();
    }

    value
}

fn patterns() -> Vec<(Regex, &'static str)> {
    vec![
        (
            Regex::new(r"(?i)(authorization:\s*bearer\s+)[A-Za-z0-9._~+/=-]+").unwrap(),
            "$1[REDACTED]",
        ),
        (
            Regex::new(
                r#"(?i)(api[_-]?key|token|secret|password|passwd|pwd)(\s*[:=]\s*)[^\s,;"']+"#,
            )
            .unwrap(),
            "$1$2[REDACTED]",
        ),
        (
            Regex::new(r"(?i)(set-cookie:\s*)[^\r\n]+").unwrap(),
            "$1[REDACTED]",
        ),
        (
            Regex::new(r#"(postgres|postgresql|mysql|mongodb|redis)://[^\s"';]+"#).unwrap(),
            "$1://[REDACTED]",
        ),
        (Regex::new(r"sk-[A-Za-z0-9]{20,}").unwrap(), "sk-[REDACTED]"),
        (
            Regex::new(r"github_pat_[A-Za-z0-9_]{20,}").unwrap(),
            "github_pat_[REDACTED]",
        ),
        (Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(), "AKIA[REDACTED]"),
        (
            Regex::new(r#"(?i)([?&](access_token|token|key|secret|password)=)[^&\s"';]+"#).unwrap(),
            "$1[REDACTED]",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_common_secret_shapes() {
        let input = "DATABASE_URL=postgres://user:pass@localhost/db token=abc123 Authorization: Bearer secret-value";
        let output = redact(input);
        assert!(output.contains("postgres://[REDACTED]"));
        assert!(output.contains("token=[REDACTED]"));
        assert!(output.contains("Bearer [REDACTED]"));
        assert!(!output.contains("user:pass"));
        assert!(!output.contains("abc123"));
        assert!(!output.contains("secret-value"));
    }
}
