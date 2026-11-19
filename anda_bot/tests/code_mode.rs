#![cfg(feature = "code-mode")]

use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn send(child: &mut Child, message: Value) {
    let bytes = serde_json::to_vec(&message).unwrap();
    let out = child.stdin.as_mut().unwrap();
    out.write_all(&[1]).unwrap();
    out.write_all(&(bytes.len() as u32).to_be_bytes()).unwrap();
    out.write_all(&bytes).unwrap();
    out.flush().unwrap();
}
fn read(child: &mut Child) -> Value {
    let stream = child.stdout.as_mut().unwrap();
    let mut header = [0u8; 5];
    stream
        .read_exact(&mut header)
        .expect("isolated worker returned a protocol frame");
    assert_eq!(header[0], 1);
    let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
    assert!(len <= 1024 * 1024);
    let mut data = vec![0; len];
    stream.read_exact(&mut data).unwrap();
    serde_json::from_slice(&data).unwrap()
}
fn start(request: Value) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_anda"))
        .arg("--internal-code-mode-worker")
        .env_clear()
        .env("MIMALLOC_DISALLOW_ARENA_ALLOC", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    send(&mut child, request);
    child
}
fn execute(request: Value) -> Value {
    let mut child = start(request);
    let result = read(&mut child);
    assert!(child.wait().unwrap().success());
    result
}

#[test]
fn code_mode_worker_composes_paginated_calls_and_preserves_u64() {
    let mut child = start(json!({"op":"exec","source":
        "cursor = None\ntotal = 0\nwhile True:\n    page = tools.page(cursor=cursor)\n    total += sum(page['result'])\n    cursor = page.get('next_cursor')\n    if cursor is None:\n        break\nprint(total)\nreturn total"}));
    for (cursor, next, number) in [
        (Value::Null, json!("2"), 9_007_199_254_740_993u64),
        (json!("2"), json!("3"), 2),
        (json!("3"), Value::Null, 3),
    ] {
        let call = read(&mut child);
        assert_eq!(call["calls"], json!([["page",{"cursor":cursor}]]));
        send(
            &mut child,
            json!({"op":"results","items":[{"value":{"result":[number],"next_cursor":next}}]}),
        );
    }
    let result = read(&mut child);
    assert_eq!(result["value"]["value"], 9_007_199_254_740_998u64);
    assert_eq!(result["value"]["stdout"], "9007199254740998\n");
    assert!(child.wait().unwrap().success());
}

#[test]
fn code_mode_worker_validates_declarations_and_defaults_without_running_the_function() {
    let source = "def count(n: int = 20, label: str | None = None) -> int:\n    \"\"\"Count entries.\"\"\"\n    return tools.count(n=n)";
    let declaration = execute(json!({"op":"inspect","source":source}));
    assert_eq!(
        declaration["value"]["name"], "script__count",
        "{declaration}"
    );
    assert_eq!(
        declaration["value"]["defaults"],
        json!({"n":20,"label":null})
    );
    assert_eq!(
        declaration["value"]["parameters"]["properties"]["label"]["type"],
        json!(["string", "null"])
    );
    for ret in ["None", "dict", "dict[str, int]", "list[dict] | None"] {
        let annotated = source.replace("-> int:", &format!("-> {ret}:"));
        assert_eq!(
            execute(json!({"op":"inspect","source":annotated}))["op"],
            "done",
            "expected return annotation {ret} to be accepted"
        );
    }
    for bad in ["import json\n", "@tools.decorator\n"] {
        assert_eq!(
            execute(json!({"op":"inspect","source":format!("{bad}{source}")}))["code"],
            "invalid_source"
        );
    }
    for bad in ["tools.count()", "[]", "'twenty'"] {
        assert_eq!(
            execute(json!({"op":"inspect","source":source.replace("= 20",&format!("= {bad}"))}))["code"],
            "invalid_source"
        );
    }
}

#[test]
fn code_mode_worker_limits_output_and_imports() {
    for source in ["import os", "import socket", "open('data.txt')"] {
        assert_eq!(
            execute(json!({"op":"exec","source":source}))["code"],
            "script_error"
        );
    }
    let result = execute(
        json!({"op":"exec","source":"import json\nprint('x' * 6000)\nreturn json.loads('[1,2]')"}),
    );
    assert_eq!(result["value"]["value"], json!([1, 2]), "{result}");
    assert_eq!(result["value"]["stdout"].as_str().unwrap().len(), 4096);
    assert_eq!(result["value"]["stdout_truncated"], true);
    let multiline = execute(json!({"op":"exec","source":"return \"\"\"first\nsecond\"\"\""}));
    assert_eq!(multiline["value"]["value"], "first\nsecond", "{multiline}");
    let escaped_stdout =
        execute(json!({"op":"exec","source":"print('\\n' * 4096)\nreturn 'x' * 30000"}));
    assert_eq!(escaped_stdout["op"], "done", "{escaped_stdout}");
    assert_eq!(
        escaped_stdout["value"]["value"].as_str().unwrap().len(),
        30000
    );
    assert_eq!(
        execute(json!({"op":"exec","source":"return 'x' * 50000"}))["code"],
        "resource_limit"
    );
}

#[test]
fn code_mode_worker_cpu_budget_terminates_a_busy_script() {
    let mut child = start(json!({"op":"exec","source":"while True:\n    pass"}));
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("worker exceeded its CPU budget")
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn code_mode_platform_denies_native_host_io() {
    let sentinel = tempfile::NamedTempFile::new().unwrap();
    assert!(std::fs::File::open(sentinel.path()).is_ok());
    assert_eq!(
        execute(json!({"op":"check_isolation","file":sentinel.path()})),
        json!({"op":"isolated"})
    );
}

#[test]
fn code_mode_worker_refuses_allocations_above_budget() {
    let child = start(
        json!({"op":"exec","source":"buffer = bytearray(320 * 1024 * 1024)\nreturn len(buffer)"}),
    );
    let output = child.wait_with_output().unwrap();
    if output.status.success() {
        assert!(output.stdout.len() >= 5);
        let response: Value = serde_json::from_slice(&output.stdout[5..]).unwrap();
        assert_eq!(
            response["op"], "failed",
            "worker exceeded its memory budget: {response}"
        );
    }
}
