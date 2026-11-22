//! The worker has no engine, identity, filesystem mounts or daemon connection.
//! Its only host capability is the framed stdin/stdout tool protocol.
use super::sandbox;
use anda_core::BoxError;
use rustpython_vm::{Interpreter, PyObjectRef, PyResult, VirtualMachine, py_serde};
use serde_json::{Value, json};
use std::io::{self, Read, Write};

pub(super) const FRAME_LIMIT: usize = 1024 * 1024;
pub(super) const VERSION: u8 = 1;
pub(super) const OUTPUT_LIMIT: usize = 32 * 1024;
pub(super) const SOURCE_LIMIT: usize = 64 * 1024;
pub(crate) const ARG: &str = "--internal-code-mode-worker";
pub(super) const ALLOCATOR_OPTION: &str = "MIMALLOC_DISALLOW_ARENA_ALLOC";

fn read_frame() -> Result<Value, BoxError> {
    let mut input = io::stdin().lock();
    let mut version = [0];
    input.read_exact(&mut version)?;
    if version != [VERSION] {
        return Err("unsupported Code Mode protocol version".into());
    }
    let mut size = [0; 4];
    input.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size > FRAME_LIMIT {
        return Err("IPC frame exceeds the byte limit".into());
    }
    let mut data = vec![0; size];
    input.read_exact(&mut data)?;
    Ok(serde_json::from_slice(&data)?)
}

fn write_frame(value: &Value) -> Result<(), BoxError> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > FRAME_LIMIT {
        return Err("IPC frame exceeds the byte limit".into());
    }
    let mut out = io::stdout().lock();
    out.write_all(&[VERSION])?;
    out.write_all(&(bytes.len() as u32).to_be_bytes())?;
    out.write_all(&bytes)?;
    out.flush()?;
    Ok(())
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedBuffer {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if b.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("JSON exceeds the byte limit"));
        }
        self.bytes.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode(vm: &VirtualMachine, obj: &PyObjectRef, limit: usize) -> Result<Vec<u8>, BoxError> {
    let mut writer = LimitedBuffer {
        bytes: Vec::new(),
        limit,
    };
    py_serde::serialize(vm, obj, &mut serde_json::Serializer::new(&mut writer))?;
    Ok(writer.bytes)
}

fn py_error(
    vm: &VirtualMachine,
    message: impl ToString,
) -> rustpython_vm::builtins::PyBaseExceptionRef {
    vm.new_value_error(message.to_string())
}

fn from_json(value: Value, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
    py_serde::deserialize(vm, value).map_err(|e| py_error(vm, e))
}

fn exchange(request: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
    let bytes = encode(vm, &request, FRAME_LIMIT).map_err(|e| py_error(vm, e))?;
    let message: Value = serde_json::from_slice(&bytes).map_err(|e| py_error(vm, e))?;
    write_frame(&message).map_err(|e| py_error(vm, e))?;
    loop {
        let reply = read_frame().map_err(|e| py_error(vm, e))?;
        match reply["op"].as_str() {
            Some("results") => return from_json(reply["items"].clone(), vm),
            Some("invoke" | "inspect") => {
                let result = evaluate(vm, reply);
                write_frame(&result).map_err(|e| py_error(vm, e))?;
            }
            _ => return Err(py_error(vm, "unexpected worker response")),
        }
    }
}

fn loads(s: String, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
    let value = serde_json::from_str(&s).map_err(|e| py_error(vm, e))?;
    from_json(value, vm)
}
fn dumps(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<String> {
    String::from_utf8(encode(vm, &value, FRAME_LIMIT).map_err(|e| py_error(vm, e))?)
        .map_err(|e| py_error(vm, e))
}

fn oversized(vm: &VirtualMachine, envelope: &PyObjectRef) -> Value {
    use rustpython_vm::builtins::{PyDict, PyList, PyStr, PyTuple};
    let value = envelope
        .downcast_ref::<PyDict>()
        .and_then(|dict| dict.get_item("value", vm).ok());
    let (kind, items) = value.as_ref().map_or(("value", None), |value| {
        if let Some(v) = value.downcast_ref::<PyList>() {
            ("array", Some(v.borrow_vec().len()))
        } else if let Some(v) = value.downcast_ref::<PyTuple>() {
            ("array", Some(v.as_slice().len()))
        } else if let Some(v) = value.downcast_ref::<PyDict>() {
            ("object", Some(v.size().used))
        } else if value.downcast_ref::<PyStr>().is_some() {
            ("string", None)
        } else {
            ("value", None)
        }
    });
    json!({"op":"failed","code":"resource_limit","message":"final JSON exceeds 32 KiB",
        "output_shape":{"type":kind,"items":items,"bytes_at_least":OUTPUT_LIMIT+1}})
}

fn evaluate(vm: &VirtualMachine, request: Value) -> Value {
    let inspecting = request["op"] == "inspect";
    let result: PyResult<PyObjectRef> = (|| {
        let scope = vm.new_scope_with_builtins();
        scope.globals.set_item(
            "_exchange",
            vm.new_function("_exchange", exchange).into(),
            vm,
        )?;
        scope
            .globals
            .set_item("_loads", vm.new_function("_loads", loads).into(), vm)?;
        scope
            .globals
            .set_item("_dumps", vm.new_function("_dumps", dumps).into(), vm)?;
        // Pure computation modules built into the VM. Scripts reach them
        // only through the guest's import allowlist.
        for name in ["itertools", "_operator", "_functools", "_collections"] {
            let module = rustpython_vm::import::import_builtin(vm, name)?;
            scope.globals.set_item(name, module, vm)?;
        }
        let code = vm
            .compile(
                include_str!("guest.py"),
                rustpython_vm::compiler::Mode::Exec,
                "<anda>",
            )
            .map_err(|e| e.into_pyexception(vm, None))?;
        vm.run_code_obj(code, scope.clone())?;
        let f = scope.globals.get_item("evaluate", vm)?;
        f.call((from_json(request, vm)?,), vm)
    })();
    match result {
        Ok(obj) => {
            let tool_error = obj
                .downcast_ref::<rustpython_vm::builtins::PyDict>()
                .and_then(|dict| dict.get_item("tool_error", vm).ok());
            if let Some(error) = tool_error {
                if let Ok(bytes) = encode(vm, &error, FRAME_LIMIT / 2)
                    && let Ok(error) = serde_json::from_slice::<Value>(&bytes)
                {
                    return json!({"op":"failed","code":error["code"],"message":error["message"],"data":error["data"]});
                }
                let dict = error.downcast_ref::<rustpython_vm::builtins::PyDict>();
                let field = |key| {
                    dict.and_then(|d| d.get_item(key, vm).ok())
                        .and_then(|v| encode(vm, &v, 4096).ok())
                        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                };
                return json!({
                    "op": "failed",
                    "code": field("code").unwrap_or_else(|| json!("tool_error")),
                    "message": field("message").unwrap_or_else(|| json!("tool failed")),
                    "data": Value::Null,
                });
            }
            let bound = if inspecting {
                FRAME_LIMIT / 2
            } else {
                // Worst-case JSON escaping expands 4 KiB stdout up to 24 KiB.
                OUTPUT_LIMIT + 32 * 1024
            };
            match encode(vm, &obj, bound).and_then(|b| Ok(serde_json::from_slice::<Value>(&b)?)) {
                Ok(value)
                    if inspecting
                        || serde_json::to_vec(&value["value"])
                            .is_ok_and(|b| b.len() <= OUTPUT_LIMIT) =>
                {
                    json!({"op":"done","value":value})
                }
                Ok(_) => oversized(vm, &obj),
                Err(e) if e.to_string().contains("JSON exceeds the byte limit") => {
                    oversized(vm, &obj)
                }
                Err(e) => json!({"op":"failed","code":"script_error","message":e.to_string()}),
            }
        }
        Err(e) => {
            use rustpython_vm::AsObject;
            if e.fast_isinstance(vm.ctx.exceptions.memory_error) {
                return json!({"op":"failed","code":"resource_limit","message":"worker memory budget exceeded"});
            }
            let mut message = String::new();
            let _ = vm.write_exception(&mut message, &e);
            message.truncate(message.floor_char_boundary(4000));
            json!({"op":"failed","code":if inspecting {"invalid_source"}else{"script_error"},"message":message})
        }
    }
}

pub(crate) fn main() -> Result<(), BoxError> {
    #[cfg(unix)]
    if std::env::var_os(ALLOCATOR_OPTION).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Err("Code Mode worker requires its bounded allocator launch configuration".into());
    }
    let executable = std::env::current_exe()?;
    if let Err(err) = sandbox::enter() {
        let _ = write_frame(
            &json!({"op":"failed","code":"sandbox_unavailable","message":err.to_string()}),
        );
        return Err(err);
    }
    let request = read_frame()?;
    if request["op"] == "check_isolation" {
        // The host supplies a readable, empty sentinel outside the worker cwd.
        // Only the result of these checks crosses the boundary.
        let path = request["file"]
            .as_str()
            .ok_or("missing isolation sentinel")?;
        let file_denied = std::fs::File::open(path).is_err();
        #[cfg(windows)]
        let network_denied = sandbox::network_denied();
        #[cfg(not(windows))]
        let network_denied = std::net::TcpListener::bind(("127.0.0.1", 0)).is_err();
        let process_denied = std::process::Command::new(executable)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_err();
        return write_frame(
            &json!({"op":if file_denied && network_denied && process_denied {"isolated"}else{"isolation_failed"}}),
        );
    }
    let execute = move || {
        let vm = Interpreter::without_stdlib(Default::default());
        let result = vm.enter(|vm| evaluate(vm, request));
        write_frame(&result)
    };
    #[cfg(windows)]
    {
        // RustPython's import initialization needs more than the PE main thread's stack.
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(execute)?
            .join()
            .map_err(|_| "Code Mode interpreter thread panicked")?
    }
    #[cfg(not(windows))]
    execute()
}
