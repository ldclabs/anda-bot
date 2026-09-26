use std::process::Command;

#[test]
fn config_validation_reads_stdin_without_initializing_a_home() {
    use std::io::Write;
    use std::process::Stdio;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("not-initialized");
    for (content, valid) in [
        (include_str!("../assets/config.yaml"), true),
        ("model: [unterminated", false),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_anda"))
            .arg("--home")
            .arg(&home)
            .arg("validate-config")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.success(), valid);
        assert!(!home.exists());
        if valid {
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim(),
                "{\"valid\":true}"
            );
        }
    }
}

#[test]
fn invalid_arguments_fail_before_initializing_a_home_or_daemon() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("not-initialized");
    for args in [
        vec!["agent", "run"],
        vec![
            "agent",
            "run",
            "--prompt",
            "hello",
            "--prompt-file",
            "prompt.txt",
        ],
        vec!["voice", "--record-secs", "0"],
        vec!["channel", "init", "wechat", "--all"],
        vec!["update", "--check", "--check-if-due"],
        vec!["update", "--skills", "--check"],
        vec!["update", "--json"],
        vec!["memory", "inbox", "--cursor", "page-two", "setup"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_anda"))
            .arg("--home")
            .arg(&home)
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(!home.exists(), "invalid command initialized home: {args:?}");
    }
}
