# Frozen Python modules for Code Mode

The worker compiles these sources into its binary with
`rustpython_vm::py_freeze!`, so scripts can use `json`, `re`, `datetime`,
`collections`, `functools` and `heapq` without file access.

All files except `warnings.py` are copied unchanged from the `Lib`
directory of `rustpython-pylib` 0.6.0, the release that matches
`rustpython-vm`. They are under the Python Software Foundation License; see
`PSF-LICENSE`. `warnings.py` is Anda's stub: the upstream module needs the
native `_contextvars`, which the worker does not include.

The set is the import closure of `json`, `re` and `datetime`, plus `heapq` for
`Counter.most_common()`. Modules such as `_sre`, `_abc`, `itertools`,
`time` and `copyreg` are built into the VM.

When upgrading RustPython, copy the same files from the new
`rustpython-pylib` release. `re` must match the VM's `_sre`:
`re/_constants.py` checks its `MAGIC`. If a module gains a new import at
module level, add that module or the import fails at run time.
