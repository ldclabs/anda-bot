"""Host-supplied bootstrap for one isolated RustPython worker."""

# The host supplies only a private IPC function and bounded JSON conversion.
# Native permissions and run accounting live outside the interpreter.
class ToolError(Exception):
    def __init__(self, error):
        super().__init__(error["message"])
        self.code = error["code"]
        self.message = error["message"]
        self.data = error.get("data")


def type_schema(node):
    kind = type(node).__name__
    if kind == "Name" and node.id in ("str", "int", "float", "bool"):
        return {"type": {"str": "string", "int": "integer", "float": "number", "bool": "boolean"}[node.id]}
    if kind == "Subscript" and getattr(node.value, "id", None) == "list":
        return {"type": "array", "items": type_schema(node.slice)}
    if kind == "BinOp" and type(node.op).__name__ == "BitOr":
        for value, none in ((node.left, node.right), (node.right, node.left)):
            if type(none).__name__ == "Constant" and none.value is None:
                schema = type_schema(value)
                if isinstance(schema["type"], str):
                    schema["type"] = [schema["type"], "null"]
                    return schema
    raise ValueError("unsupported annotation at line " + str(getattr(node, "lineno", 1)))


def literal(node, schema):
    if type(node).__name__ == "Constant":
        value = node.value
    elif (type(node).__name__ == "UnaryOp" and type(node.op).__name__ in ("USub", "UAdd")
          and type(node.operand).__name__ == "Constant" and type(node.operand.value) in (int, float)):
        value = -node.operand.value if type(node.op).__name__ == "USub" else node.operand.value
    else:
        raise ValueError("use an immutable literal default at line " + str(node.lineno))
    types = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
    matches = {str: "string", int: "integer", float: "number", bool: "boolean", type(None): "null"}
    if matches.get(type(value)) not in types and not (type(value) is int and "number" in types):
        raise ValueError("default does not match annotation at line " + str(node.lineno))
    if type(value) is float and not (-float("inf") < value < float("inf")):
        raise ValueError("default must be finite")
    return value


def inspect_source(source):
    module = compile(source, "<registered>", "exec", 1024)
    if len(module.body) != 1 or type(module.body[0]).__name__ != "FunctionDef":
        extra = next((node for node in module.body if type(node).__name__ != "FunctionDef"),
                     module.body[-1] if module.body else None)
        line = getattr(extra, "lineno", 1)
        raise ValueError(f"<registered>:{line}: declare one top-level def; put imports and helpers inside its body")
    fn = module.body[0]
    args = fn.args
    if fn.decorator_list or getattr(fn, "type_params", []) or args.posonlyargs or args.vararg or args.kwarg:
        raise ValueError(f"<registered>:{fn.lineno}: use an undecorated function with named arguments")
    if not (fn.name.isascii() and fn.name.islower() and fn.name[0].isalpha() and len(fn.name) <= 56):
        raise ValueError(f"<registered>:{fn.lineno}: use a lowercase function name of at most 56 characters")
    first = fn.body[0]
    doc = getattr(getattr(first, "value", None), "value", None)
    if type(first).__name__ != "Expr" or type(doc) is not str or not doc.strip() or len(doc) > 1600:
        raise ValueError(f"<registered>:{first.lineno}: provide a short descriptive docstring")
    params = list(args.args) + list(args.kwonlyargs)
    defaults = [None] * (len(args.args)-len(args.defaults)) + list(args.defaults) + list(args.kw_defaults)
    props, allowed_defaults, signature = {}, {}, []
    for arg, default in zip(params, defaults):
        if arg.annotation is None:
            raise ValueError(f"<registered>:{arg.lineno}: add a type annotation for {arg.arg}")
        schema = type_schema(arg.annotation)
        spec = arg.arg + ": " + py_type(schema)
        if default is not None:
            value = literal(default, schema)
            allowed_defaults[arg.arg] = value
            spec += " = " + repr(value)
            schema["description"] = "Python default: " + repr(value)
        props[arg.arg] = schema
        signature.append(spec)
    returns = py_type(type_schema(fn.returns)) if fn.returns is not None else "JsonValue"
    # Verify the body also compiles, without executing definitions or defaults.
    compile(source, "<registered>", "exec")
    return {"name": "script__" + fn.name, "description": doc.strip(),
            "signature": "(" + ", ".join(signature) + ") -> " + returns, "defaults": allowed_defaults,
            "parameters": {"type": "object", "properties": props, "required": list(props), "additionalProperties": False}}


def py_type(schema):
    names = {"string": "str", "integer": "int", "number": "float", "boolean": "bool", "null": "None"}
    types = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
    return " | ".join("list[" + py_type(schema["items"]) + "]" if t == "array" else names[t] for t in types)


class Tools:
    def __getattr__(self, name):
        if name.startswith("_"):
            raise AttributeError(name)
        return self[name]

    def __getitem__(self, name):
        def call(**args):
            item = _call_batch([(name, args)], False)[0]
            if isinstance(item, ToolError):
                raise item
            return item
        return call


def _call_batch(calls, batch):
    items = _exchange({"op": "calls", "batch": batch, "calls": calls})
    results = []
    for item in items:
        if "error" in item:
            results.append(ToolError(item["error"]))
        else:
            results.append(item["value"])
    return results


def parallel(calls):
    return _call_batch(list(calls), True)


_stdout = bytearray()
_truncated = False


def capture(*args, sep=" ", end="\n"):
    global _truncated
    data = (sep.join(str(arg) for arg in args) + end).encode()
    remaining = 4096 - len(_stdout)
    _stdout.extend(data[:remaining])
    _truncated = _truncated or len(data) > remaining


def import_data(name, globals=None, locals=None, fromlist=(), level=0):
    if level == 0 and name == "json":
        return _json_module
    raise ImportError("Code Mode provides tools and the json data module")


class JsonModule:
    loads = staticmethod(_loads)
    dumps = staticmethod(_dumps)


_json_module = JsonModule()


def evaluate(request):
    try:
        return _evaluate(request)
    except ToolError as error:
        return {"tool_error": {"code": error.code, "message": error.message, "data": error.data}}


def _evaluate(request):
    if request["op"] == "inspect":
        return inspect_source(request["source"])
    safe = dict(__builtins__ if isinstance(__builtins__, dict) else vars(__builtins__))
    for name in ("open", "input", "breakpoint", "help"):
        safe.pop(name, None)
    safe.update({"__import__": import_data, "print": capture})
    scope = {"__builtins__": safe, "__name__": "__anda__", "tools": Tools(), "parallel": parallel, "ToolError": ToolError}
    source = request["source"]
    if request["op"] == "invoke":
        if inspect_source(source)["name"] != request["name"]:
            raise ValueError("saved function name does not match its declaration")
        exec(compile(source, "<saved>", "exec"), scope)
        value = scope[request["name"][8:]](**request["args"])
    else:
        wrapped = "def _cell():\n" + "\n".join("    " + line for line in source.split("\n")) + "\n    pass"
        exec(compile(wrapped, "<code_exec>", "exec"), scope)
        value = scope["_cell"]()
    return {"value": value, "stdout": _stdout.decode("utf-8", "ignore"), "stdout_truncated": _truncated}
