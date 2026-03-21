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
    let mut f = fs::File::create(&file_path).unwrap();
    f.write_all(&vec![0xABu8; 4096 * 100]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["touch", file_path.to_str().unwrap()])
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
    let mut f = fs::File::create(&file_path).unwrap();
    f.write_all(&vec![0xABu8; 4096 * 10]).unwrap();
    f.flush().unwrap();

    let output = pagers_bin()
        .args(["evict", file_path.to_str().unwrap()])
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
        let mut f = fs::File::create(&file_path).unwrap();
        f.write_all(&vec![0u8; 4096]).unwrap();
    }

    let output = pagers_bin()
        .args(["query", dir.path().to_str().unwrap()])
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
    let mut f = fs::File::create(&file_path).unwrap();
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
    let mut f = fs::File::create(&file_path).unwrap();
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
    let mut f = fs::File::create(&file_path).unwrap();
    f.write_all(&vec![0u8; 4096]).unwrap();

    let output = pagers_bin()
        .args(["query", "-q", file_path.to_str().unwrap()])
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
    fs::write(&small, vec![0u8; 100]).unwrap();

    let large = dir.path().join("large.dat");
    fs::write(&large, vec![0u8; 100_000]).unwrap();

    let output = pagers_bin()
        .args(["query", "-m", "1k", dir.path().to_str().unwrap()])
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
    fs::write(&file_path, vec![0xABu8; 4096 * 50]).unwrap();

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
    assert!(
        stdout.contains("ResidentPercent=100"),
        "expected 100% resident, got: {stdout}"
    );
}

fn build_out_dir() -> std::path::PathBuf {
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_pagers"));
    let profile_dir = bin.parent().unwrap();
    let build_dir = profile_dir.join("build");
    for entry in fs::read_dir(&build_dir).expect("build dir not found") {
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
    fs::write(&file_path, vec![0xABu8; 4096 * 50]).unwrap();

    let output = pagers_bin()
        .args(["touch", file_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = pagers_bin()
        .args(["evict", file_path.to_str().unwrap()])
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
    fs::File::create(&file_path).unwrap();

    let output = pagers_bin()
        .args(["query", file_path.to_str().unwrap()])
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
        .args(["query", "/nonexistent/path/file.dat"])
        .output()
        .unwrap();

    let _ = output.status;
}

#[test]
fn test_query_with_range() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs::write(&file_path, vec![0u8; 4096 * 100]).unwrap();

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
    fs::write(dir.path().join("keep.txt"), vec![0u8; 4096]).unwrap();
    fs::write(dir.path().join("skip.log"), vec![0u8; 4096]).unwrap();

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
    fs::write(dir.path().join("data.bin"), vec![0u8; 4096]).unwrap();
    fs::write(dir.path().join("notes.txt"), vec![0u8; 4096]).unwrap();

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

#[test]
fn test_touch_json_output() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.dat");
    fs::write(&file_path, vec![0u8; 4096 * 10]).unwrap();

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
    fs::write(&file_path, vec![0u8; 4096 * 10]).unwrap();

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
    fs::write(&f1, vec![0u8; 4096]).unwrap();
    fs::write(&f2, vec![0u8; 4096]).unwrap();

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
fn test_completions_zsh() {
    let dir = build_out_dir();
    let content =
        fs::read_to_string(dir.join("_pagers")).expect("zsh completion file not generated");
    assert!(content.contains("#compdef pagers"), "content: {content}");
}

#[test]
fn test_completions_bash() {
    let dir = build_out_dir();
    let content =
        fs::read_to_string(dir.join("pagers.bash")).expect("bash completion file not generated");
    assert!(content.contains("pagers"), "content: {content}");
}
