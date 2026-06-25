use crate::detect::{self, DetectedStart, Severity};
use crate::redact;
use crate::store::Store;
use anyhow::{Context, Result, bail};
#[cfg(unix)]
use portable_pty::MasterPty;
use portable_pty::{ChildKiller, CommandBuilder, PtySize, native_pty_system};
use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

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
    prefer_pty: bool,
) -> Result<i32> {
    if command.is_empty() {
        bail!("capture requires a command");
    }

    let command_text = command.join(" ");
    let cwd = std::env::current_dir()?.display().to_string();
    let source = infer_source(&requested_source, &command, Path::new(&cwd));
    if nested_capture_reuses_parent_source(&source) {
        return run_uncaptured(&command);
    }
    let run_id = match store.start_run(&source, &command_text, &cwd) {
        Ok(run_id) => run_id,
        Err(err) => {
            eprintln!("RunAware capture unavailable: {err:#}. Running command without capture.");
            return run_uncaptured(&command);
        }
    };
    let use_pty = prefer_pty && stdio_is_interactive();

    let result = if use_pty {
        capture_with_pty(store, &run_id, &source, &command)
    } else {
        capture_with_pipes(store, &run_id, &source, &command)
    };

    match result {
        Ok(code) => {
            if let Err(err) = store.finish_run(&run_id, code) {
                eprintln!("RunAware failed to finish captured run: {err:#}");
            }
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
            if let Err(finish_err) = store.finish_run(&run_id, 1) {
                eprintln!("RunAware failed to finish failed captured run: {finish_err:#}");
            }
            Err(err)
        }
    }
}

pub fn run_uncaptured(command: &[String]) -> Result<i32> {
    if command.is_empty() {
        bail!("capture requires a command");
    }

    let status = Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to spawn '{}'", command.join(" ")))?;

    Ok(status.code().unwrap_or(1))
}

fn stdio_is_interactive() -> bool {
    std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
}

fn capture_with_pty(store: &Store, run_id: &str, source: &str, command: &[String]) -> Result<i32> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(current_pty_size())?;

    let mut cmd = CommandBuilder::new(&command[0]);
    cmd.args(&command[1..]);
    cmd.cwd(std::env::current_dir()?);
    cmd.env("RUNAWARE_CAPTURE_ACTIVE", "1");
    cmd.env("RUNAWARE_CAPTURE_SOURCE", source);

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .with_context(|| format!("failed to spawn '{}'", command.join(" ")))?;
    if let Some(pid) = child.process_id() {
        store.set_run_pid(run_id, pid)?;
    }
    let interrupt = InterruptGuard::install(child.process_id(), true, child.clone_killer())?;
    drop(pair.slave);

    let _terminal_mode = TerminalModeGuard::enable_for_capture()?;
    let stdin_forwarder_done = Arc::new(AtomicBool::new(false));
    let resize_forwarder_done = Arc::new(AtomicBool::new(false));
    let _stdin_forwarder_guard = DoneGuard::new(Arc::clone(&stdin_forwarder_done));
    let _resize_forwarder_guard = DoneGuard::new(Arc::clone(&resize_forwarder_done));
    let _resize_forwarder =
        spawn_resize_forwarder(pair.master.as_ref(), Arc::clone(&resize_forwarder_done));
    let _stdin_forwarder = spawn_stdin_forwarder(
        pair.master.take_writer()?,
        Arc::clone(&stdin_forwarder_done),
    );
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
    Ok(if interrupt.was_triggered() {
        130
    } else {
        status.exit_code() as i32
    })
}

struct DoneGuard {
    done: Arc<AtomicBool>,
}

impl DoneGuard {
    fn new(done: Arc<AtomicBool>) -> Self {
        Self { done }
    }
}

impl Drop for DoneGuard {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
    }
}

fn spawn_stdin_forwarder(
    mut writer: Box<dyn Write + Send>,
    done: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || forward_stdin_to_pty(&mut writer, &done))
}

#[cfg(unix)]
fn forward_stdin_to_pty(writer: &mut dyn Write, done: &AtomicBool) {
    let mut buffer = [0_u8; 8192];

    while !done.load(Ordering::SeqCst) {
        let mut poll_fd = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll_fd, 1, 100) };
        if ready == 0 {
            continue;
        }
        if ready < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if poll_fd.revents & libc::POLLNVAL != 0 {
            break;
        }
        if poll_fd.revents & libc::POLLIN == 0 {
            if poll_fd.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                break;
            }
            continue;
        }

        let n = unsafe {
            libc::read(
                libc::STDIN_FILENO,
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
            )
        };
        if n == 0 {
            break;
        }
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }

        let bytes = &buffer[..n as usize];
        if writer.write_all(bytes).is_err() {
            break;
        }
        if writer.flush().is_err() {
            break;
        }
    }
}

#[cfg(not(unix))]
fn forward_stdin_to_pty(writer: &mut dyn Write, done: &AtomicBool) {
    let mut stdin = std::io::stdin();
    let mut buffer = [0_u8; 8192];

    while !done.load(Ordering::SeqCst) {
        match stdin.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if writer.write_all(&buffer[..n]).is_err() {
                    break;
                }
                if writer.flush().is_err() {
                    break;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn current_pty_size() -> PtySize {
    current_terminal_size().unwrap_or(PtySize {
        rows: 30,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    })
}

#[cfg(unix)]
fn current_terminal_size() -> Option<PtySize> {
    for fd in [libc::STDOUT_FILENO, libc::STDIN_FILENO, libc::STDERR_FILENO] {
        let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
        let ok = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, size.as_mut_ptr()) } == 0;
        if !ok {
            continue;
        }

        let size = unsafe { size.assume_init() };
        if size.ws_row > 0 && size.ws_col > 0 {
            return Some(PtySize {
                rows: size.ws_row,
                cols: size.ws_col,
                pixel_width: size.ws_xpixel,
                pixel_height: size.ws_ypixel,
            });
        }
    }

    None
}

#[cfg(not(unix))]
fn current_terminal_size() -> Option<PtySize> {
    None
}

#[cfg(unix)]
fn spawn_resize_forwarder(
    master: &dyn MasterPty,
    done: Arc<AtomicBool>,
) -> Option<thread::JoinHandle<()>> {
    let fd = master.as_raw_fd()?;
    Some(thread::spawn(move || {
        let mut last_size = None;
        while !done.load(Ordering::SeqCst) {
            if let Some(size) = current_terminal_size()
                && last_size != Some(size)
            {
                set_pty_size(fd, size);
                last_size = Some(size);
            }
            thread::sleep(Duration::from_millis(200));
        }
    }))
}

#[cfg(not(unix))]
fn spawn_resize_forwarder(
    _master: &(dyn portable_pty::MasterPty + Send),
    _done: Arc<AtomicBool>,
) -> Option<thread::JoinHandle<()>> {
    None
}

#[cfg(unix)]
fn set_pty_size(fd: libc::c_int, size: PtySize) {
    let window_size = libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.pixel_width,
        ws_ypixel: size.pixel_height,
    };

    unsafe {
        let _ = libc::ioctl(fd, libc::TIOCSWINSZ, &window_size);
    }
}

#[cfg(unix)]
struct TerminalModeGuard {
    fd: libc::c_int,
    original: Option<libc::termios>,
}

#[cfg(unix)]
impl TerminalModeGuard {
    fn enable_for_capture() -> Result<Self> {
        let fd = libc::STDIN_FILENO;
        if unsafe { libc::isatty(fd) } != 1 {
            return Ok(Self { fd, original: None });
        }

        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to read terminal mode before PTY capture");
        }

        let original = unsafe { original.assume_init() };
        let mut raw = original;
        unsafe {
            libc::cfmakeraw(&mut raw);
        }

        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to enable raw terminal mode for PTY capture");
        }

        Ok(Self {
            fd,
            original: Some(original),
        })
    }
}

#[cfg(unix)]
impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        if let Some(original) = &self.original {
            unsafe {
                let _ = libc::tcsetattr(self.fd, libc::TCSANOW, original);
            }
        }
    }
}

#[cfg(not(unix))]
struct TerminalModeGuard;

#[cfg(not(unix))]
impl TerminalModeGuard {
    fn enable_for_capture() -> Result<Self> {
        Ok(Self)
    }
}

fn capture_with_pipes(
    store: &Store,
    run_id: &str,
    source: &str,
    command: &[String],
) -> Result<i32> {
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .env("RUNAWARE_CAPTURE_ACTIVE", "1")
        .env("RUNAWARE_CAPTURE_SOURCE", source)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn '{}'", command.join(" ")))?;
    store.set_run_pid(run_id, child.id())?;
    let interrupt = InterruptGuard::install(Some(child.id()), false, child.clone_killer())?;

    let (tx, rx) = mpsc::channel();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(spawn_pipe_reader("stdout", stdout, tx.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(spawn_pipe_reader("stderr", stderr, tx.clone()));
    }
    drop(tx);

    let mut block = None;
    let mut stdout_partial = String::new();
    let mut stderr_partial = String::new();
    for event in rx {
        match event {
            PipeEvent::Chunk { stream, bytes } => {
                process_pipe_chunk(
                    store,
                    run_id,
                    source,
                    stream,
                    &bytes,
                    if stream == "stderr" {
                        &mut stderr_partial
                    } else {
                        &mut stdout_partial
                    },
                    &mut block,
                )?;
            }
            PipeEvent::ReadError { stream, error } => {
                bail!("failed to read {stream} from captured command: {error}");
            }
        }
    }

    for reader in readers {
        reader.join().expect("pipe reader thread panicked");
    }

    if !stdout_partial.is_empty() {
        process_line(store, run_id, source, "stdout", &stdout_partial, &mut block)?;
    }
    if !stderr_partial.is_empty() {
        process_line(store, run_id, source, "stderr", &stderr_partial, &mut block)?;
    }
    flush_block(store, run_id, source, &mut block)?;

    let status = child.wait()?;
    Ok(if interrupt.was_triggered() {
        130
    } else {
        status.code().unwrap_or(1)
    })
}

enum PipeEvent {
    Chunk {
        stream: &'static str,
        bytes: Vec<u8>,
    },
    ReadError {
        stream: &'static str,
        error: std::io::Error,
    },
}

fn spawn_pipe_reader<R>(
    stream: &'static str,
    mut reader: R,
    tx: mpsc::Sender<PipeEvent>,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if tx
                        .send(PipeEvent::Chunk {
                            stream,
                            bytes: buffer[..n].to_vec(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let _ = tx.send(PipeEvent::ReadError { stream, error });
                    break;
                }
            }
        }
    })
}

fn process_pipe_chunk(
    store: &Store,
    run_id: &str,
    source: &str,
    stream: &str,
    bytes: &[u8],
    partial: &mut String,
    block: &mut Option<OpenBlock>,
) -> Result<()> {
    if stream == "stderr" {
        std::io::stderr().write_all(bytes)?;
        std::io::stderr().flush()?;
    } else {
        std::io::stdout().write_all(bytes)?;
        std::io::stdout().flush()?;
    }
    let text = String::from_utf8_lossy(bytes);
    partial.push_str(&text);
    flush_complete_lines(store, run_id, source, stream, partial, block)
}

struct InterruptGuard {
    triggered: Arc<AtomicBool>,
}

impl InterruptGuard {
    fn install(
        pid: Option<u32>,
        terminate_process_group: bool,
        killer: Box<dyn ChildKiller + Send + Sync>,
    ) -> Result<Self> {
        let triggered = Arc::new(AtomicBool::new(false));
        let handler_triggered = Arc::clone(&triggered);
        let killer = Arc::new(Mutex::new(killer));
        let handler_killer = Arc::clone(&killer);

        ctrlc::set_handler(move || {
            handler_triggered.store(true, Ordering::SeqCst);

            if terminate_process_group {
                terminate_group(pid);
            }

            if let Ok(mut killer) = handler_killer.lock() {
                let _ = killer.kill();
            }
        })
        .context("failed to install interrupt handler")?;

        Ok(Self { triggered })
    }

    fn was_triggered(&self) -> bool {
        self.triggered.load(Ordering::SeqCst)
    }
}

#[cfg(unix)]
fn terminate_group(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    if pid > i32::MAX as u32 {
        return;
    }

    unsafe {
        let pgid = -(pid as i32);
        let _ = libc::kill(pgid, libc::SIGINT);
        let _ = libc::kill(pgid, libc::SIGHUP);
    }
}

#[cfg(not(unix))]
fn terminate_group(_pid: Option<u32>) {}

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

fn nested_capture_reuses_parent_source(source: &str) -> bool {
    if std::env::var_os("RUNAWARE_CAPTURE_ACTIVE").is_none()
        || std::env::var_os("RUNAWARE_ALLOW_NESTED").is_none()
    {
        return false;
    }

    std::env::var("RUNAWARE_CAPTURE_SOURCE").is_ok_and(|parent_source| parent_source == source)
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
