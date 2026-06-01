use crate::detect::{self, DetectedStart, Severity};
use crate::redact;
use crate::store::Store;
use anyhow::{Context, Result, bail};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
struct OpenBlock {
    severity: Severity,
    title: String,
    lines: Vec<String>,
}

pub fn capture_command(
    store: &Store,
    requested_source: String,
    command: Vec<String>,
    use_pty: bool,
) -> Result<i32> {
    if command.is_empty() {
        bail!("capture requires a command");
    }

    let command_text = command.join(" ");
    let cwd = std::env::current_dir()?.display().to_string();
    let source = infer_source(&requested_source, &command, Path::new(&cwd));
    let run_id = store.start_run(&source, &command_text, &cwd)?;

    let result = if use_pty {
        capture_with_pty(store, &run_id, &source, &command)
    } else {
        capture_with_pipes(store, &run_id, &source, &command)
    };

    match result {
        Ok(code) => {
            store.finish_run(&run_id, code)?;
            Ok(code)
        }
        Err(err) => {
            store.insert_error_block(
                &run_id,
                &source,
                Severity::Fatal,
                "RunAware capture failed",
                &redact::redact(&err.to_string()),
            )?;
            store.finish_run(&run_id, 1)?;
            Err(err)
        }
    }
}

fn capture_with_pty(store: &Store, run_id: &str, source: &str, command: &[String]) -> Result<i32> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 30,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new(&command[0]);
    cmd.args(&command[1..]);
    cmd.cwd(std::env::current_dir()?);

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .with_context(|| format!("failed to spawn '{}'", command.join(" ")))?;
    if let Some(pid) = child.process_id() {
        store.set_run_pid(run_id, pid)?;
    }
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let mut stdout = std::io::stdout();
    let mut buffer = [0_u8; 8192];
    let mut partial = String::new();
    let mut block: Option<OpenBlock> = None;

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        stdout.write_all(&buffer[..n])?;
        stdout.flush()?;

        let text = String::from_utf8_lossy(&buffer[..n]);
        partial.push_str(&text);
        flush_complete_lines(store, run_id, source, "pty", &mut partial, &mut block)?;
    }

    if !partial.is_empty() {
        process_line(store, run_id, source, "pty", &partial, &mut block)?;
    }
    flush_block(store, run_id, source, &mut block)?;

    let status = child.wait()?;
    Ok(status.exit_code() as i32)
}

fn capture_with_pipes(
    store: &Store,
    run_id: &str,
    source: &str,
    command: &[String],
) -> Result<i32> {
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn '{}'", command.join(" ")))?;
    store.set_run_pid(run_id, child.id())?;

    let mut block = None;
    if let Some(mut stdout) = child.stdout.take() {
        read_pipe(store, run_id, source, "stdout", &mut stdout, &mut block)?;
    }
    if let Some(mut stderr) = child.stderr.take() {
        read_pipe(store, run_id, source, "stderr", &mut stderr, &mut block)?;
    }
    flush_block(store, run_id, source, &mut block)?;

    Ok(child.wait()?.code().unwrap_or(1))
}

fn read_pipe(
    store: &Store,
    run_id: &str,
    source: &str,
    stream: &str,
    reader: &mut impl Read,
    block: &mut Option<OpenBlock>,
) -> Result<()> {
    let mut buffer = [0_u8; 8192];
    let mut partial = String::new();
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        if stream == "stderr" {
            std::io::stderr().write_all(&buffer[..n])?;
            std::io::stderr().flush()?;
        } else {
            std::io::stdout().write_all(&buffer[..n])?;
            std::io::stdout().flush()?;
        }
        let text = String::from_utf8_lossy(&buffer[..n]);
        partial.push_str(&text);
        flush_complete_lines(store, run_id, source, stream, &mut partial, block)?;
    }
    if !partial.is_empty() {
        process_line(store, run_id, source, stream, &partial, block)?;
    }
    Ok(())
}

fn flush_complete_lines(
    store: &Store,
    run_id: &str,
    source: &str,
    stream: &str,
    partial: &mut String,
    block: &mut Option<OpenBlock>,
) -> Result<()> {
    while let Some(pos) = partial.find('\n') {
        let mut line = partial[..pos].to_string();
        if line.ends_with('\r') {
            line.pop();
        }
        process_line(store, run_id, source, stream, &line, block)?;
        *partial = partial[pos + 1..].to_string();
    }
    Ok(())
}

fn process_line(
    store: &Store,
    run_id: &str,
    source: &str,
    stream: &str,
    raw_line: &str,
    block: &mut Option<OpenBlock>,
) -> Result<()> {
    let line = redact::redact(raw_line);
    let level = detect::classify_line(&line);
    let tags = detect::tags_for(&line);
    store.insert_log(run_id, source, stream, level, &line, &tags)?;

    if let Some(open) = block.as_mut() {
        if detect::is_continuation(&line) {
            open.lines.push(line);
            return Ok(());
        }
        flush_block(store, run_id, source, block)?;
    }

    if let Some(DetectedStart { severity, title }) = detect::detect_start(&line) {
        let single_line_warning = severity == Severity::Warning && !line.contains("warning:");
        *block = Some(OpenBlock {
            severity,
            title,
            lines: vec![line],
        });
        if single_line_warning {
            flush_block(store, run_id, source, block)?;
        }
    }

    Ok(())
}

fn flush_block(
    store: &Store,
    run_id: &str,
    source: &str,
    block: &mut Option<OpenBlock>,
) -> Result<()> {
    if let Some(open) = block.take() {
        let body = open.lines.join("\n");
        store.insert_error_block(run_id, source, open.severity, &open.title, &body)?;
    }
    Ok(())
}

fn infer_source(requested: &str, command: &[String], cwd: &Path) -> String {
    if requested != "auto" {
        return requested.to_string();
    }

    if let Ok(source) = std::env::var("RUNAWARE_SOURCE") {
        if !source.trim().is_empty() {
            return source;
        }
    }

    let text = command.join(" ").to_lowercase();

    if is_package_manager_command(command) {
        if let Some(package_name) = package_name_from_cwd(cwd) {
            return package_name;
        }
        if let Some(dir_name) = cwd.file_name().and_then(|value| value.to_str()) {
            return sanitize_source(dir_name);
        }
    }

    if text.contains("test") || text.contains("pytest") || text.contains("cargo test") {
        "tests".to_string()
    } else if text.contains("worker") || text.contains("queue") || text.contains("sidekiq") {
        "worker".to_string()
    } else if text.contains("api") || text.contains("server") || text.contains("backend") {
        "api".to_string()
    } else if text.contains("vite")
        || text.contains("next")
        || text.contains("frontend")
        || text.contains("web")
    {
        "frontend".to_string()
    } else if text.starts_with("docker") {
        "docker".to_string()
    } else {
        command
            .first()
            .map(|value| sanitize_source(value))
            .unwrap_or_else(|| "process".to_string())
    }
}

fn is_package_manager_command(command: &[String]) -> bool {
    let Some(executable) = command.first().map(|value| value.as_str()) else {
        return false;
    };
    matches!(executable, "npm" | "pnpm" | "yarn" | "bun")
}

fn package_name_from_cwd(cwd: &Path) -> Option<String> {
    let package_json = cwd.join("package.json");
    let raw = std::fs::read_to_string(package_json).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let name = value.get("name")?.as_str()?;
    let short_name = name.rsplit('/').next().unwrap_or(name);
    let source = sanitize_source(short_name);
    (!source.is_empty()).then_some(source)
}

fn sanitize_source(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::infer_source;

    #[test]
    fn infers_package_name_for_package_manager_commands() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{ "name": "@acme/api-service" }"#,
        )
        .unwrap();

        let source = infer_source(
            "auto",
            &["pnpm".to_string(), "run".to_string(), "dev".to_string()],
            dir.path(),
        );

        assert_eq!(source, "api-service");
    }

    #[test]
    fn falls_back_to_directory_name_for_package_manager_commands() {
        let dir = tempfile::tempdir().unwrap();
        let source = infer_source(
            "auto",
            &["pnpm".to_string(), "install".to_string()],
            dir.path(),
        );

        assert!(!source.is_empty());
        assert_ne!(source, "pnpm");
    }
}
