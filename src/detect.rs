use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Info => write!(f, "info"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
    Fatal,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
            Severity::Fatal => write!(f, "fatal"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetectedStart {
    pub severity: Severity,
    pub title: String,
}

pub fn classify_line(line: &str) -> LogLevel {
    let lower = line.to_lowercase();
    if lower.contains("fatal")
        || lower.contains("panic")
        || lower.contains("traceback")
        || lower.contains("exception")
        || lower.contains("error:")
        || lower.contains(" error ")
        || lower.contains("eaddrinuse")
        || lower.contains("econnrefused")
        || lower.contains("failed")
        || lower.contains(" 500 ")
        || lower.contains(" 5xx ")
    {
        LogLevel::Error
    } else if lower.contains("warn")
        || lower.contains("deprecated")
        || lower.contains(" 400 ")
        || lower.contains(" 404 ")
        || lower.contains(" 4xx ")
    {
        LogLevel::Warn
    } else {
        LogLevel::Info
    }
}

pub fn detect_start(line: &str) -> Option<DetectedStart> {
    let lower = line.to_lowercase();
    let severity =
        if lower.contains("fatal") || lower.contains("panic") || lower.contains("crashed") {
            Some(Severity::Fatal)
        } else if classify_line(line) == LogLevel::Error {
            Some(Severity::Error)
        } else if classify_line(line) == LogLevel::Warn {
            Some(Severity::Warning)
        } else {
            None
        }?;

    Some(DetectedStart {
        severity,
        title: title_for(line),
    })
}

pub fn is_continuation(line: &str) -> bool {
    let trimmed = line.trim_start();
    if line.starts_with(' ') || line.starts_with('\t') {
        return true;
    }

    let patterns = [
        r"^at\s+",
        r"^File\s+",
        r"^Traceback",
        r"^Caused by:",
        r"^\.\.\.",
        r"^npm ERR!",
        r"^yarn ERR!",
        r"^pnpm ERR!",
        r"^thread '.*' panicked",
        r"^\d+\)\s+",
        r"^Expected",
        r"^Received",
        r"^Diff:",
    ];

    patterns
        .iter()
        .any(|pattern| Regex::new(pattern).unwrap().is_match(trimmed))
}

pub fn tags_for(line: &str) -> Vec<String> {
    let lower = line.to_lowercase();
    let mut tags = Vec::new();

    for (needle, tag) in [
        ("eaddrinuse", "port-conflict"),
        ("address already in use", "port-conflict"),
        ("econnrefused", "network"),
        ("connection refused", "network"),
        ("cannot find module", "dependency"),
        ("module_not_found", "dependency"),
        ("traceback", "python"),
        ("prisma", "database"),
        ("database", "database"),
        ("migration", "database"),
        ("500", "http-5xx"),
        ("404", "http-4xx"),
        ("failed", "failure"),
        ("test failed", "test"),
    ] {
        if lower.contains(needle) {
            tags.push(tag.to_string());
        }
    }

    tags
}

fn title_for(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= 140 {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..140])
    }
}

#[cfg(test)]
mod tests {
    use super::{LogLevel, Severity, classify_line, detect_start, is_continuation};

    #[test]
    fn detects_error_and_stack_continuation() {
        assert_eq!(classify_line("Error: Cannot find module"), LogLevel::Error);
        let start = detect_start("Error: Cannot find module").unwrap();
        assert_eq!(start.severity, Severity::Error);
        assert!(is_continuation("  at resolver.js:1"));
    }

    #[test]
    fn detects_warning() {
        assert_eq!(classify_line("warning: deprecated API"), LogLevel::Warn);
        let start = detect_start("warning: deprecated API").unwrap();
        assert_eq!(start.severity, Severity::Warning);
    }
}
