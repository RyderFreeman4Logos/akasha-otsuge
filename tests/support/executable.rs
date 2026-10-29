use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub fn write(path: &Path, contents: &str) {
    // Keep writable script FDs out of the multithreaded test process and its forks.
    let mut writer = Command::new("/usr/bin/python3")
        .args(["-c", "import os,sys\nwith open(sys.argv[1], 'wb') as f: f.write(sys.stdin.buffer.read())\nos.chmod(sys.argv[1], 0o700)"])
        .arg(path).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped())
        .spawn().unwrap();
    writer
        .stdin
        .take()
        .unwrap()
        .write_all(contents.as_bytes())
        .unwrap();
    let output = writer.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "fixture writer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
