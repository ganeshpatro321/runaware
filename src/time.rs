use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Utc};

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

pub fn parse_since(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(ts) = DateTime::parse_from_rfc3339(value) {
        return Ok(ts.with_timezone(&Utc));
    }

    let value = value.trim();
    if value.is_empty() {
        bail!("since value cannot be empty");
    }

    let (amount, unit) = value.split_at(value.len() - 1);
    let amount: i64 = amount.parse().map_err(|_| {
        anyhow::anyhow!("invalid since value '{value}', expected 10m, 2h, 1d, or RFC3339")
    })?;

    let duration = match unit {
        "s" => Duration::seconds(amount),
        "m" => Duration::minutes(amount),
        "h" => Duration::hours(amount),
        "d" => Duration::days(amount),
        _ => bail!("invalid since unit '{unit}', expected s, m, h, or d"),
    };

    Ok(Utc::now() - duration)
}

#[cfg(test)]
mod tests {
    use super::parse_since;

    #[test]
    fn parses_relative_time() {
        assert!(parse_since("10m").is_ok());
        assert!(parse_since("2h").is_ok());
        assert!(parse_since("1d").is_ok());
    }
}
