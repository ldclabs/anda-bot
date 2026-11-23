#![cfg(feature = "code-mode")]

#[cfg(windows)]
#[path = "../src/engine/code_mode/windows_profile.rs"]
mod windows_profile;

use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Worker {
    child: Child,
    #[cfg(windows)]
    _profile: windows_profile::Profile,
}

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
fn start(request: Value) -> Worker {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anda"));
    command
        .arg("--internal-code-mode-worker")
        .env_clear()
        .env("MIMALLOC_DISALLOW_ARENA_ALLOC", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    let profile = {
        let profile = windows_profile::Profile::create().unwrap();
        profile.configure(&mut command);
        profile
    };
    let mut worker = Worker {
        child: command.spawn().unwrap(),
        #[cfg(windows)]
        _profile: profile,
    };
    send(&mut worker.child, request);
    worker
}
fn execute(request: Value) -> Value {
    let mut worker = start(request);
    let child = &mut worker.child;
    let result = read(child);
    assert!(child.wait().unwrap().success());
    result
}

#[test]
fn code_mode_worker_composes_paginated_calls_and_preserves_u64() {
    let mut worker = start(json!({"op":"exec","source":
        "cursor = None\ntotal = 0\nwhile True:\n    page = tools.page(cursor=cursor)\n    total += sum(page['result'])\n    cursor = page.get('next_cursor')\n    if cursor is None:\n        break\nprint(total)\nreturn total"}));
    let child = &mut worker.child;
    for (cursor, next, number) in [
        (Value::Null, json!("2"), 9_007_199_254_740_993u64),
        (json!("2"), json!("3"), 2),
        (json!("3"), Value::Null, 3),
    ] {
        let call = read(child);
        assert_eq!(call["calls"], json!([["page",{"cursor":cursor}]]));
        send(
            child,
            json!({"op":"results","items":[{"value":{"result":[number],"next_cursor":next}}]}),
        );
    }
    let result = read(child);
    assert_eq!(result["value"]["value"], 9_007_199_254_740_998u64);
    assert_eq!(result["value"]["stdout"], "9007199254740998\n");
    assert!(child.wait().unwrap().success());
}

#[test]
fn code_mode_worker_returns_parallel_errors_as_data() {
    let mut worker = start(json!({"op":"exec","source":
        "parallel([('page', {'cursor': None}), ('page', {'cursor': '2'})])"}));
    let child = &mut worker.child;
    let call = read(child);
    assert_eq!(call["batch"], true, "{call}");
    send(
        child,
        json!({"op":"results","items":[{"value":{"result":[1]}},{"error":{"code":"denied","message":"no"}}]}),
    );
    let result = read(child);
    assert_eq!(
        result["value"]["value"],
        json!([{"result":[1]},{"error":{"code":"denied","message":"no","data":null}}]),
        "{result}"
    );
    assert!(child.wait().unwrap().success());
}

#[test]
fn code_mode_worker_imports_re_math_and_datetime() {
    let result = execute(json!({"op":"exec","source":
        "import re, math, heapq\nfrom datetime import date, datetime, timedelta, timezone\nfrom collections import Counter, namedtuple\nfrom functools import lru_cache\nPoint = namedtuple('Point', 'x y')\n@lru_cache\ndef square(n):\n    return n * n\nstamp = datetime(2024, 3, 1, 10, 30, tzinfo=timezone.utc)\n{'ids': re.findall(r'#(\\d+)', 'fix #12 and #345'), 'swap': re.sub(r'(\\w+)@(\\w+)', r'\\2 at \\1', 'me@home', 1), 'pages': math.ceil(7 / 3), 'root': math.isqrt(17), 'due': date(2024, 2, 28) + timedelta(days=2), 'stamp': stamp, 'label': stamp.strftime('%a %H:%M'), 'epoch': datetime.utcfromtimestamp(0), 'point': Point(1, 2)._asdict(), 'top': Counter('abracadabra').most_common(2), 'square': square(12), 'small': heapq.nsmallest(2, [5, 1, 4])}"}));
    // A positional re.sub() count and utcfromtimestamp() warn upstream; the
    // worker drops the warnings.
    assert_eq!(
        result["value"]["value"],
        json!({"ids":["12","345"],"swap":"home at me","pages":3,"root":4,"due":"2024-03-01","stamp":"2024-03-01T10:30:00+00:00","label":"Fri 10:30","epoch":"1970-01-01T00:00:00","point":{"x":1,"y":2},"top":[["a",5],["b",2]],"square":144,"small":[1,4]}),
        "{result}"
    );
    // Dates passed to tools become ISO strings as well.
    let mut worker = start(json!({"op":"exec","source":
        "from datetime import date\nreturn tools.page(since=date(2024, 1, 2))"}));
    let child = &mut worker.child;
    assert_eq!(
        read(child)["calls"],
        json!([["page",{"since":"2024-01-02"}]])
    );
    send(
        child,
        json!({"op":"results","items":[{"value":{"ok":true}}]}),
    );
    assert_eq!(read(child)["value"]["value"], json!({"ok":true}));
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
    for source in [
        "import os",
        "import socket",
        "import ctypes",
        "import _ctypes",
        "open('data.txt')",
    ] {
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
    // The pure modules built into the VM; a final expression is the result.
    let modules = execute(json!({"op":"exec","source":
        "from collections import Counter, defaultdict, deque, OrderedDict\nimport itertools, operator\nfrom functools import reduce, partial\ncounts = Counter(['b', 'a', 'b'])\ncounts.update({'c': 1}, a=2)\ngroups = defaultdict(list)\nfor key, items in itertools.groupby(sorted([3, 1, 2], key=operator.neg), key=lambda n: n % 2):\n    groups[key].extend(items)\n{'common': counts.most_common(2), 'groups': groups, 'sum': reduce(operator.add, deque([1, 2, 3])), 'ids': Counter([7, 7]), 'set': {1}, 'ordered': OrderedDict(a=1), 'add': partial(operator.add, 1)(2)}"}));
    assert_eq!(
        modules["value"]["value"],
        json!({"common":[["a",3],["b",2]],"groups":{"1":[3,1],"0":[2]},"sum":6,"ids":{"7":2},"set":[1],"ordered":{"a":1},"add":3}),
        "{modules}"
    );
    for (source, message) in [
        ("import time", "can import only"),
        ("import sys", "can import only"),
        ("tools.page(1)", "keyword arguments only"),
        ("parallel([tools.page])", "(tool_name, kwargs) pairs"),
    ] {
        let failed = execute(json!({"op":"exec","source":source}));
        assert_eq!(failed["code"], "script_error", "{failed}");
        assert!(
            failed["message"].as_str().unwrap().contains(message),
            "{failed}"
        );
    }
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
fn code_mode_worker_honors_json_and_print_options() {
    let result = execute(json!({"op":"exec","source":r#"
import json
from json import JSONDecodeError

class Buffer:
    def __init__(self):
        self.parts = []
        self.flushed = False
    def write(self, part):
        self.parts.append(part)
    def flush(self):
        self.flushed = True

buffer = Buffer()
print('a', 'b', sep=None, end=None, file=buffer, flush=True)
try:
    json.loads('{')
except JSONDecodeError as error:
    print(error.lineno, error.colno, flush=True)
data = {'z': '中', 'a': 1}
pretty = json.dumps(data, indent=2, ensure_ascii=False, sort_keys=True)
return [pretty, json.dumps({'k': '中'}), json.loads(pretty.encode(), parse_int=str), ''.join(buffer.parts), buffer.flushed]
"#}));
    assert_eq!(
        result["value"]["value"],
        json!(["{\n  \"a\": 1,\n  \"z\": \"中\"\n}", "{\"k\":\"中\"}", {"a":"1","z":"中"}, "a b\n", true]),
        "{result}"
    );
    assert_eq!(result["value"]["stdout"], "1 2\n");
}

#[test]
fn code_mode_worker_cpu_budget_terminates_a_busy_script() {
    let mut worker = start(json!({"op":"exec","source":"while True:\n    pass"}));
    let child = &mut worker.child;
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
    let worker = start(
        json!({"op":"exec","source":"buffer = bytearray(320 * 1024 * 1024)\nreturn len(buffer)"}),
    );
    let output = worker.child.wait_with_output().unwrap();
    if output.status.success() {
        assert!(output.stdout.len() >= 5);
        let response: Value = serde_json::from_slice(&output.stdout[5..]).unwrap();
        assert_eq!(
            response["op"], "failed",
            "worker exceeded its memory budget: {response}"
        );
    }
}
