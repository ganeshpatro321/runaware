#[cfg(unix)]
#[test]
fn pty_capture_forwards_enter_to_child_stdin() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::{Read, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let data_dir = tempfile::tempdir().unwrap();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_runaware"));
    cmd.args([
        "capture",
        "--source",
        "test",
        "--",
        "sh",
        "-c",
        "printf READY; IFS= read line; printf 'KEY:ENTER\\n'",
    ]);
    cmd.cwd(env!("CARGO_MANIFEST_DIR"));
    cmd.env("RUNAWARE_HOME", data_dir.path().as_os_str());

    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = String::new();
    let mut sent_enter = false;

    while Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            output.push_str(&String::from_utf8_lossy(&chunk));
        }
        if output.contains("READY") && !sent_enter {
            writer.write_all(b"\r").unwrap();
            writer.flush().unwrap();
            sent_enter = true;
        }
        if output.contains("KEY:ENTER") {
            let status = child.wait().unwrap();
            assert_eq!(status.exit_code(), 0, "{output:?}");
            return;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("runaware exited before forwarding Enter: status={status:?} output={output:?}");
        }
    }

    child.kill().ok();
    child.wait().ok();
    panic!("timed out waiting for Enter to reach child, output={output:?}");
}

#[cfg(unix)]
#[test]
fn capture_uses_pipe_mode_when_stdout_is_not_a_terminal() {
    let data_dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_runaware"))
        .args([
            "capture",
            "--source",
            "test",
            "--",
            "sh",
            "-c",
            "if [ -t 1 ]; then printf tty; else printf pipe; fi",
        ])
        .env("RUNAWARE_HOME", data_dir.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "pipe");
}

#[cfg(unix)]
#[test]
fn pipe_capture_forwards_piped_stdin() {
    use std::io::Write;
    use std::process::Stdio;

    let data_dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_runaware"))
        .args(["capture", "--source", "test", "--", "sh", "-c", "cat"])
        .env("RUNAWARE_HOME", data_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hello through stdin\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "hello through stdin\n"
    );
}

#[cfg(unix)]
#[test]
fn pipe_capture_reads_stdout_and_stderr_concurrently() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let data_dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_runaware"))
        .args([
            "capture",
            "--no-pty",
            "--source",
            "test",
            "--",
            "sh",
            "-c",
            "dd if=/dev/zero bs=262144 count=1 2>/dev/null | tr '\\000' x >&2; printf done",
        ])
        .env("RUNAWARE_HOME", data_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(String::from_utf8_lossy(&output.stdout), "done");
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    child.kill().ok();
    child.wait().ok();
    panic!("pipe capture deadlocked while child wrote heavily to stderr");
}

#[cfg(unix)]
#[test]
fn capture_falls_back_to_uncaptured_command_when_store_cannot_open() {
    let data_dir = tempfile::tempdir().unwrap();
    let home_file = data_dir.path().join("not-a-directory");
    std::fs::write(&home_file, "not a directory").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_runaware"))
        .args([
            "capture",
            "--source",
            "test",
            "--",
            "sh",
            "-c",
            "printf fallback",
        ])
        .env("RUNAWARE_HOME", home_file)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "fallback");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("RunAware capture unavailable"),
        "{output:?}"
    );
}
