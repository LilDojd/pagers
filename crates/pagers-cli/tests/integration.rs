use std::fs;
use std::io::Write;
use std::process::Command;

fn pagers_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pagers"))
}

#[test]
fn test_query_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs::File::create(&file_path).unwrap();
    f.write_all(&vec![0u8; 4096 * 10]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["query", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files: 1"), "stdout: {stdout}");
    assert!(stdout.contains("Resident Pages:"), "stdout: {stdout}");
}

#[test]
fn test_touch_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs::File::create(&file_path).unwrap();
    f.write_all(&vec![0xABu8; 4096 * 100]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["touch", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Touched Pages:"), "stdout: {stdout}");
}

#[test]
fn test_evict_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs::File::create(&file_path).unwrap();
    f.write_all(&vec![0xABu8; 4096 * 10]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["evict", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Evicted Pages:"), "stdout: {stdout}");
}

#[test]
fn test_query_directory() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        let file_path = dir.path().join(format!("file{i}.dat"));
        let mut f = fs::File::create(&file_path).unwrap();
        f.write_all(&vec![0u8; 4096]).unwrap();
    }

    let output = pagers_bin()
        .args(["query", dir.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files: 5"), "stdout: {stdout}");
}

#[test]
fn test_kv_output() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs::File::create(&file_path).unwrap();
    f.write_all(&vec![0u8; 4096]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["query", "-o", "kv", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files=1"), "stdout: {stdout}");
    assert!(stdout.contains("TotalPages="), "stdout: {stdout}");
}

#[test]
fn test_quiet_mode() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    let mut f = fs::File::create(&file_path).unwrap();
    f.write_all(&vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args(["query", "-q", file_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "stdout should be empty in quiet mode");
}

#[test]
fn test_no_subcommand_shows_help() {
    let output = pagers_bin()
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage") || stderr.contains("pagers"), "stderr: {stderr}");
}

#[test]
fn test_max_file_size_filter() {
    let dir = tempfile::tempdir().unwrap();

    let small = dir.path().join("small.dat");
    fs::write(&small, &vec![0u8; 100]).unwrap();

    let large = dir.path().join("large.dat");
    fs::write(&large, &vec![0u8; 100_000]).unwrap();

    let output = pagers_bin()
        .args(["query", "-m", "1k", dir.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Files: 1"), "should only process small file, got: {stdout}");
}

#[test]
fn test_touch_then_query_shows_resident() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs::write(&file_path, &vec![0xABu8; 4096 * 50]).unwrap();

    let output = pagers_bin()
        .args(["touch", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = pagers_bin()
        .args(["query", "-o", "kv", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ResidentPercent=100"), "expected 100% resident, got: {stdout}");
}

#[test]
fn test_completions_zsh() {
    let output = pagers_bin()
        .args(["completions", "zsh"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("#compdef pagers"), "stdout: {stdout}");
}

#[test]
fn test_completions_bash() {
    let output = pagers_bin()
        .args(["completions", "bash"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pagers"), "stdout: {stdout}");
}
