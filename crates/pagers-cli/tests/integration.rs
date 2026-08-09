use std::io::Write;
use std::process::Command;

fn pagers_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pagers"))
}

fn parse_json(output: &std::process::Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\nstdout: {stdout}"))
}

#[test]
fn test_filters_apply_to_explicit_files() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("dope_dealer228.txt");
    fs_err::write(&text, vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args(["query", "-I", "*.bin", "-o", "kv", text.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Files=0"));
}

#[test]
fn test_max_size() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");

    fs_err::write(&file_path, vec![67u8; 4096]).unwrap();

    let output = pagers_bin()
        .args(["query", "-m", "1k", "-o", "kv", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Files=0"));
}

#[test]
fn test_invalid_filter_pattern_fails_cleanly() {
    let output = pagers_bin()
        .args(["query", "-I", "[z-a]", "-o", "kv", "README.md"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("invalid path pattern"), "stderr: {stderr}");
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
}

#[test]
fn test_daemon_validates_patterns_before_forking() {
    let output = pagers_bin()
        .args(["lock", "--daemon", "-I", "[z-a]", "-o", "kv", "README.md"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("invalid path pattern"), "stderr: {stderr}");
}

#[test]
fn test_daemon_wait_reports_processing_failure() {
    let file = tempfile::NamedTempFile::new().unwrap();
    fs_err::write(file.path(), vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args([
            "lock",
            "--daemon",
            "--wait",
            "-p",
            "1M..",
            "-o",
            "kv",
            file.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn test_daemon_wait_closes_captured_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lock.dat");
    let pidfile = dir.path().join("pagers.pid");
    fs_err::write(&file, vec![0u8; 4096]).unwrap();

    let child = pagers_bin()
        .args([
            "lock",
            "--daemon",
            "--wait",
            "--pidfile",
            pidfile.to_str().unwrap(),
            "-o",
            "kv",
            file.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || tx.send(child.wait_with_output()).unwrap());

    let result = rx.recv_timeout(std::time::Duration::from_secs(5));
    let pid = fs_err::read_to_string(&pidfile).unwrap_or_default();
    if !pid.trim().is_empty() {
        let _ = Command::new("kill").args(["-TERM", pid.trim()]).status();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while pidfile.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let stopped = !pidfile.exists();
    if !stopped && !pid.trim().is_empty() {
        let _ = Command::new("kill").args(["-KILL", pid.trim()]).status();
        let _ = fs_err::remove_file(&pidfile);
    }

    let output = match result {
        Ok(output) => output.unwrap(),
        Err(_) => {
            let _ = waiter.join();
            panic!("daemon kept captured standard descriptors open after readiness");
        }
    };
    waiter.join().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stopped, "daemon did not stop after SIGTERM");
}

#[cfg(target_os = "linux")]
#[test]
fn test_foreground_lock_retains_locked_mapping() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lock.dat");
    fs_err::write(&file, vec![0u8; 4096]).unwrap();

    let mut child = pagers_bin()
        .args(["lock", "-o", "kv", file.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let status_path = format!("/proc/{}/status", child.id());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let locked = loop {
        if let Ok(status) = fs_err::read_to_string(&status_path)
            && let Some(kib) = status
                .lines()
                .find_map(|line| line.strip_prefix("VmLck:"))
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse::<usize>().ok())
            && kib > 0
        {
            break true;
        }
        if child.try_wait().unwrap().is_some() || std::time::Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let _ = child.kill();
    let _ = child.wait();

    assert!(locked, "foreground lock process did not retain any locked memory");
}

#[test]
fn test_query_single_thread_completes() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0u8; 4096]).unwrap();

    let mut child = pagers_bin()
        .args(["query", "-j", "1", "-o", "kv", file_path.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("single-thread query timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn test_query_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs_err::File::create(&file_path).unwrap();
    f.write_all(&vec![0u8; 4096 * 10]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["query", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files: 1"), "stdout: {stdout}");
    assert!(stdout.contains("Resident Pages:"), "stdout: {stdout}");
}

#[test]
fn test_touch_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs_err::File::create(&file_path).unwrap();
    f.write_all(&vec![0xABu8; 4096 * 100]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["touch", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Touched Pages:"), "stdout: {stdout}");
}

#[test]
fn test_evict_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs_err::File::create(&file_path).unwrap();
    f.write_all(&vec![0xABu8; 4096 * 10]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["evict", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Evicted Pages:"), "stdout: {stdout}");
}

#[test]
fn test_query_directory() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        let file_path = dir.path().join(format!("file{i}.dat"));
        let mut f = fs_err::File::create(&file_path).unwrap();
        f.write_all(&vec![0u8; 4096]).unwrap();
    }

    let output = pagers_bin()
        .args(["query", "-o", "human", dir.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files: 5"), "stdout: {stdout}");
}

#[test]
fn test_kv_output() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs_err::File::create(&file_path).unwrap();
    f.write_all(&vec![0u8; 4096]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["query", "-o", "kv", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=1"), "stdout: {stdout}");
    assert!(stdout.contains("TotalPages="), "stdout: {stdout}");
}

#[test]
fn test_json_output() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs_err::File::create(&file_path).unwrap();
    f.write_all(&vec![0u8; 4096]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["query", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"files\":1"), "stdout: {stdout}");
    assert!(stdout.contains("\"total_pages\":"), "stdout: {stdout}");
    assert!(stdout.starts_with('{'), "should be JSON object: {stdout}");
}

#[test]
fn test_quiet_mode() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs_err::File::create(&file_path).unwrap();
    f.write_all(&vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args(["query", "-o", "human", "-q", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty in quiet mode"
    );
}

#[test]
fn test_no_subcommand_shows_help() {
    let output = pagers_bin().output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage") || stderr.contains("pagers"),
        "stderr: {stderr}"
    );
}

#[test]
fn test_max_file_size_filter() {
    let dir = tempfile::tempdir().unwrap();

    let small = dir.path().join("small.dat");
    fs_err::write(&small, vec![0u8; 100]).unwrap();

    let large = dir.path().join("large.dat");
    fs_err::write(&large, vec![0u8; 100_000]).unwrap();

    let output = pagers_bin()
        .args([
            "query",
            "-m",
            "1k",
            "-o",
            "human",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Files: 1"),
        "should only process small file, got: {stdout}"
    );
}

#[test]
fn test_touch_then_query_shows_resident() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0xABu8; 4096 * 50]).unwrap();

    let output = pagers_bin()
        .args(["touch", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = pagers_bin()
        .args(["query", "-o", "kv", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TotalResidentPercent=100"),
        "expected 100% resident, got: {stdout}"
    );
}

fn build_out_dir() -> std::path::PathBuf {
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_pagers"));
    let profile_dir = bin.parent().unwrap();
    let build_dir = profile_dir.join("build");
    for entry in fs_err::read_dir(&build_dir).expect("build dir not found") {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().starts_with("pagers-") {
            let out = entry.path().join("out");
            if out.join("_pagers").exists() {
                return out;
            }
        }
    }
    panic!("completion files not found in {}", build_dir.display());
}

#[test]
fn test_evict_then_query_runs_successfully() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0xABu8; 4096 * 50]).unwrap();

    let output = pagers_bin()
        .args(["touch", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = pagers_bin()
        .args(["evict", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Evicted Pages:"), "stdout: {stdout}");

    let output = pagers_bin()
        .args(["query", "-o", "kv", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=1"), "stdout: {stdout}");
}

#[test]
fn test_query_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("empty.dat");
    fs_err::File::create(&file_path).unwrap();

    let output = pagers_bin()
        .args(["query", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Files: 0") || stdout.contains("TotalPages=0"),
        "stdout: {stdout}"
    );
}

#[test]
fn test_query_nonexistent_file() {
    let output = pagers_bin()
        .args(["query", "-o", "human", "/nonexistent/path/file.dat"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
}

#[test]
fn test_query_range_beyond_file_fails() {
    let file = tempfile::NamedTempFile::new().unwrap();
    fs_err::write(file.path(), vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args([
            "query",
            "-p",
            "1M..",
            "-o",
            "kv",
            file.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
}

#[test]
fn test_query_with_range() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0u8; 4096 * 100]).unwrap();

    let output = pagers_bin()
        .args([
            "query",
            "-p",
            "0..100K",
            "-o",
            "kv",
            file_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=1"), "stdout: {stdout}");
}

#[test]
fn test_query_with_ignore_pattern() {
    let dir = tempfile::tempdir().unwrap();
    fs_err::write(dir.path().join("keep.txt"), vec![0u8; 4096]).unwrap();
    fs_err::write(dir.path().join("skip.log"), vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args([
            "query",
            "-i",
            "*.log",
            "-o",
            "kv",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Files=1"),
        "should skip .log file, got: {stdout}"
    );
}

#[test]
fn test_query_with_filter_pattern() {
    let dir = tempfile::tempdir().unwrap();
    fs_err::write(dir.path().join("data.bin"), vec![0u8; 4096]).unwrap();
    fs_err::write(dir.path().join("notes.txt"), vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args([
            "query",
            "-I",
            "*.bin",
            "-o",
            "kv",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Files=1"),
        "should only process .bin file, got: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn test_follow_symlinked_directory() {
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fs_err::write(target.path().join("data.bin"), vec![0u8; 4096]).unwrap();
    std::os::unix::fs::symlink(target.path(), root.path().join("linked")).unwrap();

    let without_follow = pagers_bin()
        .args(["query", "-o", "kv", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let with_follow = pagers_bin()
        .args(["query", "-f", "-o", "kv", root.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(without_follow.status.success());
    assert!(String::from_utf8_lossy(&without_follow.stdout).contains("Files=0"));
    assert!(with_follow.status.success());
    assert!(String::from_utf8_lossy(&with_follow.stdout).contains("Files=1"));
}

#[cfg(unix)]
#[test]
fn test_follow_symlink_cycle_completes() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("nested");
    fs_err::create_dir(&nested).unwrap();
    fs_err::write(nested.join("data.bin"), vec![0u8; 4096]).unwrap();
    std::os::unix::fs::symlink(root.path(), nested.join("back")).unwrap();

    let mut child = pagers_bin()
        .args(["query", "-f", "-o", "kv", root.path().to_str().unwrap()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("symlink cycle did not terminate");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn test_hardlink_policy_uses_walker_metadata() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.bin");
    let second = root.path().join("second.bin");
    fs_err::write(&first, vec![0u8; 4096]).unwrap();
    fs_err::hard_link(&first, &second).unwrap();

    let deduplicated = pagers_bin()
        .args(["query", "-o", "kv", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let counted = pagers_bin()
        .args(["query", "-H", "-o", "kv", root.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(deduplicated.status.success());
    assert!(String::from_utf8_lossy(&deduplicated.stdout).contains("Files=1"));
    assert!(counted.status.success());
    assert!(String::from_utf8_lossy(&counted.stdout).contains("Files=2"));
}

#[test]
fn test_touch_json_output() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0u8; 4096 * 10]).unwrap();

    let output = pagers_bin()
        .args(["touch", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with('{'), "expected JSON, got: {stdout}");
    assert!(
        stdout.contains("\"touched_pages\":"),
        "expected touched_ prefix in JSON, got: {stdout}"
    );
}

#[test]
fn test_evict_json_output() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0u8; 4096 * 10]).unwrap();

    let output = pagers_bin()
        .args(["evict", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with('{'), "expected JSON, got: {stdout}");
    assert!(
        stdout.contains("\"evicted_pages\":"),
        "expected evicted_ prefix in JSON, got: {stdout}"
    );
}

#[test]
fn test_query_multiple_files() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("a.dat");
    let f2 = dir.path().join("b.dat");
    fs_err::write(&f1, vec![0u8; 4096]).unwrap();
    fs_err::write(&f2, vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args([
            "query",
            "-o",
            "kv",
            f1.to_str().unwrap(),
            f2.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=2"), "stdout: {stdout}");
}

#[test]
fn test_batch_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("a.dat");
    let f2 = dir.path().join("b.dat");
    fs_err::write(&f1, vec![0u8; 4096]).unwrap();
    fs_err::write(&f2, vec![0u8; 4096]).unwrap();

    let batch_file = dir.path().join("paths.txt");
    fs_err::write(&batch_file, format!("{}\n{}\n", f1.display(), f2.display())).unwrap();

    let output = pagers_bin()
        .args(["query", "-b", batch_file.to_str().unwrap(), "-o", "kv"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=2"), "stdout: {stdout}");
}

#[test]
fn test_missing_batch_file_fails() {
    let output = pagers_bin()
        .args([
            "query",
            "-b",
            "/nonexistent/pagers-batch-file",
            "-o",
            "kv",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
}

#[test]
fn test_batch_nul_delimited() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("a.dat");
    let f2 = dir.path().join("b.dat");
    let f3 = dir.path().join("c.dat");
    fs_err::write(&f1, vec![0u8; 4096]).unwrap();
    fs_err::write(&f2, vec![0u8; 4096]).unwrap();
    fs_err::write(&f3, vec![0u8; 4096]).unwrap();

    let batch_file = dir.path().join("paths.0");
    let mut content = Vec::new();
    for f in [&f1, &f2, &f3] {
        content.extend_from_slice(f.to_str().unwrap().as_bytes());
        content.push(b'\0');
    }
    fs_err::write(&batch_file, &content).unwrap();

    let output = pagers_bin()
        .args([
            "query",
            "-b",
            batch_file.to_str().unwrap(),
            "-0",
            "-o",
            "kv",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=3"), "stdout: {stdout}");
}

#[test]
fn test_batch_combined_with_positional_args() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("positional.dat");
    let f2 = dir.path().join("batched.dat");
    fs_err::write(&f1, vec![0u8; 4096]).unwrap();
    fs_err::write(&f2, vec![0u8; 4096]).unwrap();

    let batch_file = dir.path().join("paths.txt");
    fs_err::write(&batch_file, format!("{}\n", f2.display())).unwrap();

    let output = pagers_bin()
        .args([
            "query",
            "-b",
            batch_file.to_str().unwrap(),
            "-o",
            "kv",
            f1.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=2"), "stdout: {stdout}");
}

#[test]
fn test_batch_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("a.dat");
    let f2 = dir.path().join("b.dat");
    fs_err::write(&f1, vec![0u8; 4096]).unwrap();
    fs_err::write(&f2, vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args(["query", "-b", "-", "-o", "kv"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            let stdin = child.stdin.as_mut().unwrap();
            writeln!(stdin, "{}", f1.display()).unwrap();
            writeln!(stdin, "{}", f2.display()).unwrap();
            child.wait_with_output()
        })
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=2"), "stdout: {stdout}");
}

#[test]
fn test_batch_empty_lines_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("a.dat");
    fs_err::write(&f1, vec![0u8; 4096]).unwrap();

    let batch_file = dir.path().join("paths.txt");
    fs_err::write(&batch_file, format!("\n\n{}\n\n", f1.display())).unwrap();

    let output = pagers_bin()
        .args(["query", "-b", batch_file.to_str().unwrap(), "-o", "kv"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=1"), "stdout: {stdout}");
}

#[test]
fn test_touch_reports_consistent_counts_and_resident() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0xABu8; 4096 * 50]).unwrap();
    fs_err::File::open(&file_path).unwrap().sync_all().unwrap();

    // Advise eviction first; the kernel may still retain some or all pages.
    let output = pagers_bin()
        .args(["evict", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = pagers_bin()
        .args(["touch", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json = parse_json(&output);
    let touched = json["touched_pages"].as_i64().unwrap();
    let total = json["total_pages"].as_i64().unwrap();
    let resident = json["total_resident_pages"].as_i64().unwrap();
    assert!(total > 0);
    assert!(touched <= total, "touched_pages exceeds total: {json}");
    assert_eq!(resident, total, "all pages should be resident after touch");
}

#[test]
fn test_evict_reports_measured_residency_delta() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0xABu8; 4096 * 50]).unwrap();

    // Touch first
    let output = pagers_bin()
        .args(["touch", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Evict
    let output = pagers_bin()
        .args(["evict", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = parse_json(&output);
    let evicted = json["evicted_pages"].as_i64().unwrap();
    let total = json["total_pages"].as_i64().unwrap();
    let resident = json["total_resident_pages"].as_i64().unwrap();
    assert_eq!(evicted + resident, total, "inconsistent summary: {json}");
}

#[test]
fn test_query_shows_only_resident() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0xABu8; 4096 * 20]).unwrap();

    let output = pagers_bin()
        .args(["query", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json = parse_json(&output);
    assert!(json.get("total_resident_pages").is_some());
    assert!(
        json.get("touched_pages").is_none(),
        "query should not have touched_pages"
    );
    assert!(
        json.get("evicted_pages").is_none(),
        "query should not have evicted_pages"
    );
}

#[test]
fn test_touch_then_evict_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0xABu8; 4096 * 30]).unwrap();
    fs_err::File::open(&file_path).unwrap().sync_all().unwrap();

    // Touch
    let output = pagers_bin()
        .args(["touch", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = parse_json(&output);
    let total = json["total_pages"].as_i64().unwrap();
    assert_eq!(json["total_resident_pages"].as_i64().unwrap(), total);

    // Query: 100% resident
    let output = pagers_bin()
        .args(["query", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = parse_json(&output);
    assert_eq!(json["total_resident_pages"].as_i64().unwrap(), total);

    // Evict
    let output = pagers_bin()
        .args(["evict", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = parse_json(&output);
    let resident_after_evict = json["total_resident_pages"].as_i64().unwrap();
    assert_eq!(
        json["evicted_pages"].as_i64().unwrap() + resident_after_evict,
        total,
        "inconsistent eviction summary: {json}"
    );

    // Query must agree with the measured eviction result.
    let output = pagers_bin()
        .args(["query", "-o", "json", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = parse_json(&output);
    assert_eq!(
        json["total_resident_pages"].as_i64().unwrap(),
        resident_after_evict
    );
}

#[test]
fn test_touch_directory_reports_counts() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        let path = dir.path().join(format!("f{i}.dat"));
        fs_err::write(&path, vec![0xABu8; 4096 * 10]).unwrap();
        fs_err::File::open(&path).unwrap().sync_all().unwrap();
    }

    // Advise eviction first; the kernel may still retain some or all pages.
    let output = pagers_bin()
        .args(["evict", "-o", "human", dir.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = pagers_bin()
        .args(["touch", "-o", "json", dir.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json = parse_json(&output);
    assert_eq!(json["files"].as_i64().unwrap(), 5);
    assert!(json["touched_pages"].as_i64().unwrap() <= json["total_pages"].as_i64().unwrap());
    assert_eq!(
        json["total_resident_pages"].as_i64().unwrap(),
        json["total_pages"].as_i64().unwrap()
    );
}

#[test]
fn test_touch_kv_has_both_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs_err::write(&file_path, vec![0xABu8; 4096 * 20]).unwrap();
    fs_err::File::open(&file_path).unwrap().sync_all().unwrap();

    // Advise eviction first; the kernel may still retain some or all pages.
    let output = pagers_bin()
        .args(["evict", "-o", "human", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = pagers_bin()
        .args(["touch", "-o", "kv", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TouchedPages="), "stdout: {stdout}");
    assert!(stdout.contains("TotalResidentPages="), "stdout: {stdout}");
}

#[test]
fn test_completions_zsh() {
    let dir = build_out_dir();
    let content =
        fs_err::read_to_string(dir.join("_pagers")).expect("zsh completion file not generated");
    assert!(content.contains("#compdef pagers"), "content: {content}");
}

#[test]
fn test_completions_bash() {
    let dir = build_out_dir();
    let content = fs_err::read_to_string(dir.join("pagers.bash"))
        .expect("bash completion file not generated");
    assert!(content.contains("pagers"), "content: {content}");
}
