use crate::store::{ErrorBlock, SourceInfo, Store};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSummary {
    pub window_start: String,
    pub sources: Vec<SourceInfo>,
    pub error_count: usize,
    pub warning_count: usize,
    pub latest_errors: Vec<ErrorBlock>,
    pub source_counts: BTreeMap<String, usize>,
    pub correlation_hints: Vec<String>,
    pub likely_root_cause: Option<String>,
}

pub fn summarize(
    store: &Store,
    since: DateTime<Utc>,
    source: Option<&str>,
    latest_only: bool,
) -> Result<RuntimeSummary> {
    let mut sources = store.list_sources()?;
    sources.retain(|candidate| {
        candidate.active && source.is_none_or(|requested| requested == candidate.name)
    });
    let errors = store.error_blocks_since(since, source, 25, latest_only)?;
    let mut source_counts = BTreeMap::new();
    let mut error_count = 0;
    let mut warning_count = 0;

    for error in &errors {
        *source_counts.entry(error.source.clone()).or_insert(0) += 1;
        if error.severity == "warning" {
            warning_count += 1;
        } else {
            error_count += 1;
        }
    }

    let correlation_hints = correlation_hints(&errors);
    let likely_root_cause = likely_root_cause(&errors, &correlation_hints);

    Ok(RuntimeSummary {
        window_start: since.to_rfc3339(),
        sources,
        error_count,
        warning_count,
        latest_errors: errors.into_iter().take(8).collect(),
        source_counts,
        correlation_hints,
        likely_root_cause,
    })
}

pub fn render_text(summary: &RuntimeSummary) -> String {
    let mut out = Vec::new();
    out.push(format!("Runtime window starts at {}", summary.window_start));

    if summary.sources.is_empty() {
        out.push("No runtime sources have been captured yet.".to_string());
        return out.join("\n");
    }

    out.push(format!(
        "Known sources: {}",
        summary
            .sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    out.push(format!(
        "Detected {} errors and {} warnings.",
        summary.error_count, summary.warning_count
    ));

    if let Some(root) = &summary.likely_root_cause {
        out.push(format!("Likely root cause: {root}"));
    }

    if !summary.correlation_hints.is_empty() {
        out.push("Cross-source hints:".to_string());
        for hint in &summary.correlation_hints {
            out.push(format!("- {hint}"));
        }
    }

    if summary.latest_errors.is_empty() {
        out.push("No recent errors or warnings found.".to_string());
    } else {
        out.push("Most relevant errors:".to_string());
        for error in &summary.latest_errors {
            out.push(format!(
                "- {} [{}] {}: {}",
                error.start_ts, error.source, error.severity, error.title
            ));
        }
    }

    out.join("\n")
}

fn correlation_hints(errors: &[ErrorBlock]) -> Vec<String> {
    let mut hints = Vec::new();
    for (index, current) in errors.iter().enumerate() {
        for other in errors.iter().skip(index + 1) {
            if current.source == other.source {
                continue;
            }
            let Ok(current_ts) = DateTime::parse_from_rfc3339(&current.start_ts) else {
                continue;
            };
            let Ok(other_ts) = DateTime::parse_from_rfc3339(&other.start_ts) else {
                continue;
            };
            let seconds = (current_ts.timestamp() - other_ts.timestamp()).abs();
            if seconds <= 5 && related(current, other) {
                hints.push(format!(
                    "{} error may be related to {} error within {}s: '{}' <-> '{}'",
                    current.source, other.source, seconds, current.title, other.title
                ));
            }
        }
    }
    hints.sort();
    hints.dedup();
    hints.into_iter().take(5).collect()
}

fn related(a: &ErrorBlock, b: &ErrorBlock) -> bool {
    let text = format!(
        "{} {} {} {}",
        a.title.to_lowercase(),
        a.body.to_lowercase(),
        b.title.to_lowercase(),
        b.body.to_lowercase()
    );
    text.contains("500")
        || text.contains("database")
        || text.contains("econnrefused")
        || text.contains("connection refused")
        || text.contains("api")
        || text.contains("fetch")
        || text.contains("http")
}

fn likely_root_cause(errors: &[ErrorBlock], hints: &[String]) -> Option<String> {
    let database = errors.iter().find(|error| {
        let body = error.body.to_lowercase();
        body.contains("database")
            || body.contains("prisma")
            || body.contains("migration")
            || body.contains("column")
    });
    if let Some(error) = database {
        return Some(format!(
            "{} reported a database-related failure: {}",
            error.source, error.title
        ));
    }

    let port = errors.iter().find(|error| {
        let body = error.body.to_lowercase();
        body.contains("eaddrinuse") || body.contains("address already in use")
    });
    if let Some(error) = port {
        return Some(format!(
            "{} has a port conflict: {}",
            error.source, error.title
        ));
    }

    let network = errors.iter().find(|error| {
        let body = error.body.to_lowercase();
        body.contains("econnrefused") || body.contains("connection refused")
    });
    if let Some(error) = network {
        return Some(format!(
            "{} reported a network connection failure: {}",
            error.source, error.title
        ));
    }

    if !hints.is_empty() {
        return Some(
            "Multiple sources produced related errors in the same time window.".to_string(),
        );
    }

    errors
        .first()
        .map(|error| format!("{} most recently reported: {}", error.source, error.title))
}
