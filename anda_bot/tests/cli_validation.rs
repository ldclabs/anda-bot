use std::process::Command;

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
