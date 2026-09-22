use std::process::Command;

#[test]
fn memory_guide_is_offline_and_does_not_create_home() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("not-initialized");
    let result = Command::new(env!("CARGO_BIN_EXE_anda"))
        .args(["--home", home.to_str().unwrap(), "memory", "guide"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("长期记忆"));
    assert!(!home.exists());
}

#[test]
fn memory_guide_rejects_json_before_creating_home() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("not-initialized");
    let result = Command::new(env!("CARGO_BIN_EXE_anda"))
        .args([
            "--home",
            home.to_str().unwrap(),
            "memory",
            "guide",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!home.exists());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--json"));
}
