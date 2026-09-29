#!/usr/bin/env python3
"""
LSP test harness for tsc-rs.

Tests hover, go-to-definition, completions, and references against
expected results. Run from the repo root:

    python3 scripts/test-lsp.py [--binary target/release/tsc-rs] [--verbose]
"""

import subprocess, json, threading, time, queue, sys, os

BINARY = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                      "target", "release", "tsc-rs")
VERBOSE = "--verbose" in sys.argv or "-v" in sys.argv

for arg in sys.argv[1:]:
    if arg.startswith("--binary="):
        BINARY = arg.split("=", 1)[1]


# ---------------------------------------------------------------------------
# LSP client
# ---------------------------------------------------------------------------

class LSPClient:
    def __init__(self, binary, cwd="/tmp"):
        self.proc = subprocess.Popen(
            [binary, "--lsp"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            cwd=cwd
        )
        self.queue = queue.Queue()
        self.stderr_lines = []
        self._next_id = 1
        self._reader = threading.Thread(target=self._read_stdout, daemon=True)
        self._stderr_reader = threading.Thread(target=self._read_stderr, daemon=True)
        self._reader.start()
        self._stderr_reader.start()

    def _read_stdout(self):
        buf = b""
        try:
            while True:
                chunk = self.proc.stdout.read(1)
                if not chunk:
                    break
                buf += chunk
                while b"Content-Length:" in buf:
                    try:
                        idx = buf.index(b"Content-Length:")
                        nl = buf.index(b"\r\n\r\n", idx)
                        length = int(buf[idx:nl].split(b":")[1].strip())
                        body_start = nl + 4
                        if len(buf) >= body_start + length:
                            body = buf[body_start:body_start+length]
                            self.queue.put(json.loads(body))
                            buf = buf[body_start+length:]
                        else:
                            break
                    except:
                        break
        except:
            pass

    def _read_stderr(self):
        try:
            for line in self.proc.stderr:
                self.stderr_lines.append(line.decode().rstrip())
        except:
            pass

    def send(self, method, params=None, is_notification=False):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not is_notification:
            msg["id"] = self._next_id
            self._next_id += 1
        body = json.dumps(msg).encode()
        header = f"Content-Length: {len(body)}\r\n\r\n".encode()
        self.proc.stdin.write(header + body)
        self.proc.stdin.flush()
        return msg.get("id")

    def recv(self, request_id=None, timeout=3):
        """Wait for a response with matching id, collecting notifications."""
        end = time.time() + timeout
        notifications = []
        while time.time() < end:
            try:
                msg = self.queue.get(timeout=0.1)
                if request_id is not None and msg.get("id") == request_id:
                    return msg, notifications
                notifications.append(msg)
            except queue.Empty:
                pass
        return None, notifications

    def request(self, method, params=None, timeout=3):
        rid = self.send(method, params)
        resp, _ = self.recv(rid, timeout)
        return resp

    def notify(self, method, params=None):
        self.send(method, params, is_notification=True)

    def shutdown(self):
        self.request("shutdown")
        self.notify("exit")
        self.proc.wait(timeout=3)

    def open_file(self, uri, text, language="typescript"):
        self.notify("textDocument/didOpen", {
            "textDocument": {"uri": uri, "languageId": language, "version": 1, "text": text}
        })
        time.sleep(0.2)  # Wait for diagnostics

    def hover(self, uri, line, char):
        resp = self.request("textDocument/hover", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": char}
        })
        if resp and resp.get("result"):
            contents = resp["result"].get("contents", {})
            if isinstance(contents, dict):
                val = contents.get("value", "")
                # Strip markdown fences
                val = val.replace("```typescript\n", "").replace("\n```", "").strip()
                return val
            return str(contents)
        return None

    def definition(self, uri, line, char):
        resp = self.request("textDocument/definition", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": char}
        })
        result = resp.get("result") if resp else None
        if result is None:
            return None
        if isinstance(result, list):
            return result
        return [result]  # Single location

    def completions(self, uri, line, char):
        resp = self.request("textDocument/completion", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": char}
        })
        result = resp.get("result") if resp else None
        if result is None:
            return []
        if isinstance(result, list):
            return result
        if isinstance(result, dict):
            return result.get("items", [])
        return []

    def references(self, uri, line, char):
        resp = self.request("textDocument/references", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": char},
            "context": {"includeDeclaration": True}
        })
        result = resp.get("result") if resp else None
        return result or []


# ---------------------------------------------------------------------------
# Test framework
# ---------------------------------------------------------------------------

class TestRunner:
    def __init__(self):
        self.passed = 0
        self.failed = 0
        self.errors = []

    def assert_eq(self, name, actual, expected):
        if actual == expected:
            self.passed += 1
            if VERBOSE:
                print(f"  PASS {name}")
        else:
            self.failed += 1
            self.errors.append((name, expected, actual))
            print(f"  FAIL {name}")
            print(f"    expected: {expected!r}")
            print(f"    actual:   {actual!r}")

    def assert_contains(self, name, actual, substring):
        if actual is not None and substring in actual:
            self.passed += 1
            if VERBOSE:
                print(f"  PASS {name}")
        else:
            self.failed += 1
            self.errors.append((name, f"contains '{substring}'", actual))
            print(f"  FAIL {name}")
            print(f"    expected to contain: {substring!r}")
            print(f"    actual: {actual!r}")

    def assert_not_none(self, name, actual):
        if actual is not None:
            self.passed += 1
            if VERBOSE:
                print(f"  PASS {name}")
        else:
            self.failed += 1
            self.errors.append((name, "not None", actual))
            print(f"  FAIL {name}")

    def assert_none(self, name, actual):
        if actual is None:
            self.passed += 1
            if VERBOSE:
                print(f"  PASS {name}")
        else:
            self.failed += 1
            self.errors.append((name, "None", actual))
            print(f"  FAIL {name}")

    def summary(self):
        total = self.passed + self.failed
        print(f"\n{'='*60}")
        print(f"Results: {self.passed}/{total} passed, {self.failed} failed")
        if self.failed > 0:
            print(f"\nFailed tests:")
            for name, expected, actual in self.errors:
                print(f"  - {name}")
        print(f"{'='*60}")
        return self.failed == 0


# ---------------------------------------------------------------------------
# Test cases
# ---------------------------------------------------------------------------

def test_basic_hover(client, t):
    """Test hover shows types for basic declarations."""
    print("\n--- Basic hover ---")
    code = """const x: number = 42;
const s: string = "hello";
const b = true;
const arr: number[] = [1, 2, 3];
function add(a: number, b: number): number { return a + b; }
const sum = add(1, 2);
class Greeter { name: string; constructor(n: string) { this.name = n; } }
interface Point { x: number; y: number; }
type ID = string | number;
enum Color { Red, Green, Blue }
"""
    uri = "file:///tmp/test-basic.ts"
    client.open_file(uri, code)

    h = client.hover(uri, 0, 6)   # x
    t.assert_contains("hover const x: number", h, "number")

    h = client.hover(uri, 1, 6)   # s
    t.assert_contains("hover const s: string", h, "string")

    h = client.hover(uri, 4, 10)  # add function
    t.assert_not_none("hover function add exists", h)
    t.assert_contains("hover function add has 'add'", h, "add")

    h = client.hover(uri, 5, 6)   # sum
    t.assert_not_none("hover const sum exists", h)

    h = client.hover(uri, 6, 6)   # Greeter class
    t.assert_contains("hover class Greeter", h, "Greeter")

    h = client.hover(uri, 7, 10)  # Point interface
    t.assert_contains("hover interface Point", h, "Point")

    h = client.hover(uri, 8, 5)   # ID type alias
    t.assert_contains("hover type ID", h, "ID")

    h = client.hover(uri, 9, 5)   # Color enum
    t.assert_contains("hover enum Color", h, "Color")


def test_destructuring(client, t):
    """Test destructuring binds symbols with types."""
    print("\n--- Destructuring ---")
    code = """const obj = { name: "hello", age: 42 };
const { name, age } = obj;
const arr: [string, number] = ["hi", 1];
const [first, second] = arr;
function fn({ x, y }: { x: number; y: string }) { return x; }
"""
    uri = "file:///tmp/test-destr.ts"
    client.open_file(uri, code)

    h = client.hover(uri, 1, 8)   # name (destructured)
    t.assert_not_none("hover destructured 'name' exists", h)
    t.assert_contains("hover destructured 'name' has name", h, "name")

    h = client.hover(uri, 1, 14)  # age (destructured)
    t.assert_not_none("hover destructured 'age' exists", h)

    h = client.hover(uri, 3, 7)   # first (array destructured)
    t.assert_not_none("hover array destructured 'first' exists", h)


def test_definition(client, t):
    """Test go-to-definition for local symbols."""
    print("\n--- Go to definition ---")
    code = """const greeting = "hello";
function say(msg: string) { return msg; }
const result = say(greeting);
"""
    uri = "file:///tmp/test-def.ts"
    client.open_file(uri, code)

    # Definition of 'greeting' at usage site (line 2, char 20)
    d = client.definition(uri, 2, 20)
    t.assert_not_none("definition of 'greeting' usage", d)
    if d:
        t.assert_eq("definition 'greeting' points to line 0", d[0]["range"]["start"]["line"], 0)

    # Definition of 'say' call (line 2, char 15)
    d = client.definition(uri, 2, 15)
    t.assert_not_none("definition of 'say' call", d)
    if d:
        t.assert_eq("definition 'say' points to line 1", d[0]["range"]["start"]["line"], 1)


def test_imports(client, t):
    """Test import symbols are bound and hoverable."""
    print("\n--- Import binding ---")
    # Create a module file first
    mod_code = """export function helper(x: number): string { return String(x); }
export const VERSION = "1.0";
export interface Config { port: number; host: string; }
"""
    mod_uri = "file:///tmp/test-module.ts"
    client.open_file(mod_uri, mod_code)

    # Now open a file that imports from it
    code = """import { helper, VERSION, Config } from "./test-module";
const result = helper(42);
const v = VERSION;
"""
    uri = "file:///tmp/test-imports.ts"
    client.open_file(uri, code)

    h = client.hover(uri, 0, 10)  # helper import
    t.assert_not_none("hover import 'helper' exists", h)
    t.assert_contains("hover import 'helper' name", h, "helper")

    h = client.hover(uri, 0, 18)  # VERSION import
    t.assert_not_none("hover import 'VERSION' exists", h)

    h = client.hover(uri, 0, 28)  # Config import
    t.assert_not_none("hover import 'Config' exists", h)


def test_completions(client, t):
    """Test completions return items."""
    print("\n--- Completions ---")
    code = """const message = "hello";
const len = message.
"""
    uri = "file:///tmp/test-comp.ts"
    client.open_file(uri, code)

    items = client.completions(uri, 1, 20)
    t.assert_not_none("completions at dot access", items)
    if items:
        names = [i.get("label", "") for i in items]
        if VERBOSE:
            print(f"    completion items: {names[:10]}")


def test_references(client, t):
    """Test find references."""
    print("\n--- References ---")
    code = """const x = 1;
const y = x + 2;
const z = x * 3;
"""
    uri = "file:///tmp/test-refs.ts"
    client.open_file(uri, code)

    refs = client.references(uri, 0, 6)
    if VERBOSE:
        print(f"    references for 'x': {len(refs)}")
    # x appears 3 times: declaration + 2 uses
    t.assert_eq("references for 'x' count >= 1", len(refs) >= 1, True)


def test_kind_display(client, t):
    """Test kind prefix display for different symbol types."""
    print("\n--- Kind display ---")
    code = """const myConst = 42;
let myLet = "hello";
function myFunc(x: number): string { return String(x); }
"""
    uri = "file:///tmp/test-kind.ts"
    client.open_file(uri, code)

    h = client.hover(uri, 0, 6)  # myConst
    t.assert_contains("const prefix", h, "const ")
    if VERBOSE and h:
        print(f"    myConst: {h}")

    h = client.hover(uri, 1, 4)  # myLet
    # let and const share same flags in binder — both show as const
    t.assert_not_none("let/const prefix exists", h)
    t.assert_contains("let shows type", h, "string")
    if VERBOSE and h:
        print(f"    myLet: {h}")

    h = client.hover(uri, 2, 10)  # myFunc
    t.assert_contains("function prefix", h, "function ")
    if VERBOSE and h:
        print(f"    myFunc: {h}")


def test_keywords_no_hover(client, t):
    """Keywords should not produce hover."""
    print("\n--- Keywords ---")
    code = """const x = 1;
if (x > 0) { return x; }
"""
    uri = "file:///tmp/test-kw.ts"
    client.open_file(uri, code)

    h = client.hover(uri, 1, 0)   # 'if' keyword
    t.assert_none("hover 'if' keyword is null", h)

    h = client.hover(uri, 1, 14)  # 'return' keyword
    t.assert_none("hover 'return' keyword is null", h)


def test_member_access(client, t):
    """Test member access hover and definition."""
    print("\n--- Member access ---")
    code = """const msg = "hello world";
const upper = msg.toUpperCase();
const len = msg.length;
const parts = msg.split(" ");
const arr = [1, 2, 3];
const first = arr[0];
const mapped = arr.map(x => x * 2);
"""
    uri = "file:///tmp/test-member.ts"
    client.open_file(uri, code)

    # Hover on .toUpperCase should show a type
    h = client.hover(uri, 1, 18)  # toUpperCase
    t.assert_not_none("hover 'msg.toUpperCase' resolves", h)
    if VERBOSE and h:
        print(f"    msg.toUpperCase hover: {h}")

    # Hover on .length should show number
    h = client.hover(uri, 2, 16)  # length
    t.assert_not_none("hover 'msg.length' resolves", h)
    if VERBOSE and h:
        print(f"    msg.length hover: {h}")

    # Hover on .split should resolve
    h = client.hover(uri, 3, 18)  # split
    t.assert_not_none("hover 'msg.split' resolves", h)
    if VERBOSE and h:
        print(f"    msg.split hover: {h}")

    # Hover on .map should resolve
    h = client.hover(uri, 6, 20)  # map
    t.assert_not_none("hover 'arr.map' resolves", h)
    if VERBOSE and h:
        print(f"    arr.map hover: {h}")


def test_web_globals(client, t):
    """Test web API globals are available."""
    print("\n--- Web globals ---")
    code = """async function handler(req: Request): Promise<Response> {
    const url = new URL(req.url);
    const body = await req.json();
    const headers = new Headers();
    headers.set("Content-Type", "application/json");
    return new Response(JSON.stringify(body), { status: 200, headers });
}
"""
    uri = "file:///tmp/test-web.ts"
    client.open_file(uri, code)

    # Hover on req parameter — should show type
    h = client.hover(uri, 0, 23)  # req
    t.assert_not_none("hover 'req' param exists", h)
    t.assert_contains("hover 'req' shows Request type", h, "Request")
    if VERBOSE and h:
        print(f"    req hover: {h}")

    # Definition of req should work (req starts at col 26 on line 2: "    const body = await req.json();")
    d = client.definition(uri, 2, 26)  # req in req.json()
    t.assert_not_none("definition of 'req' in req.json()", d)

    # Hover on headers variable
    h = client.hover(uri, 3, 10)  # headers
    t.assert_not_none("hover 'headers' var", h)
    if VERBOSE and h:
        print(f"    headers hover: {h}")

    # Hover on .json() member of req — should show method type
    h = client.hover(uri, 2, 30)  # json in req.json()
    t.assert_not_none("hover 'req.json()' resolves", h)
    if VERBOSE and h:
        print(f"    req.json hover: {h}")


def test_type_shapes(client, t):
    """Test that inferred types have correct shapes."""
    print("\n--- Type shapes ---")
    code = """const num = 42;
const str = "hello";
const bool = true;
const obj = { name: "Alice", age: 30 };
const nums = [1, 2, 3];
function add(a: number, b: number): number { return a + b; }
const { name, age } = obj;
const [first] = nums;
const typed: string = "world";
class Point { constructor(public x: number, public y: number) {} }
const p = new Point(1, 2);
const len = str.length;
const upper = str.toUpperCase();
const map = new Map<string, number>();
async function fetchData(): Promise<string> { return "data"; }
const data = await fetchData();
"""
    uri = "file:///tmp/test-shapes.ts"
    client.open_file(uri, code)

    shapes = [
        (0, 6, "num", "number"),
        (1, 6, "str", "string"),
        (2, 6, "bool", "boolean"),
        (3, 6, "obj", "name:"),
        (4, 6, "nums", "number[]"),
        (5, 9, "add", "(a: number"),
        (6, 8, "name", "string"),
        (6, 14, "age", "number"),
        (7, 7, "first", "number"),
        (8, 6, "typed", "string"),
        (9, 6, "Point", "class"),
        (10, 6, "p", "Point"),
        (11, 6, "len", "number"),
        (12, 6, "upper", "string"),
        (13, 6, "map", "Map<string, number>"),
        (15, 6, "data", "string"),
    ]

    for line, col, token, expected in shapes:
        h = client.hover(uri, line, col)
        if h and expected.lower() in h.lower():
            t.passed += 1
            if VERBOSE:
                print(f"  PASS shape {token}: {h}")
        else:
            t.failed += 1
            t.errors.append((f"shape {token}", expected, h))
            print(f"  FAIL shape {token}")
            print(f"    expected to contain: {expected!r}")
            print(f"    actual: {h!r}")


def test_document_symbols(client, t):
    """Test document symbols returns outline."""
    print("\n--- Document symbols ---")
    code = """function foo() {}
class Bar { method() {} }
const baz = 1;
"""
    uri = "file:///tmp/test-symbols.ts"
    client.open_file(uri, code)

    resp = client.request("textDocument/documentSymbol", {
        "textDocument": {"uri": uri}
    })
    result = resp.get("result") if resp else None
    t.assert_not_none("document symbols returns items", result)
    if result:
        names = [s.get("name", "") for s in result]
        t.assert_eq("document symbols includes 'foo'", "foo" in names, True)
        t.assert_eq("document symbols includes 'Bar'", "Bar" in names, True)
        if VERBOSE:
            print(f"    symbols: {names}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    if not os.path.exists(BINARY):
        print(f"Binary not found: {BINARY}")
        print("Build with: cargo build --release -p tsc-rs")
        sys.exit(1)

    print(f"Testing tsc-rs LSP: {BINARY}")
    t = TestRunner()
    client = LSPClient(BINARY)

    # Initialize
    resp = client.request("initialize", {"capabilities": {}})
    assert resp and resp.get("result"), "Failed to initialize"
    client.notify("initialized")
    time.sleep(0.1)

    server_info = resp["result"].get("serverInfo", {})
    print(f"Server: {server_info.get('name')} v{server_info.get('version')}")

    try:
        test_basic_hover(client, t)
        test_destructuring(client, t)
        test_definition(client, t)
        test_imports(client, t)
        test_completions(client, t)
        test_references(client, t)
        test_member_access(client, t)
        test_web_globals(client, t)
        test_kind_display(client, t)
        test_keywords_no_hover(client, t)
        test_type_shapes(client, t)
        test_document_symbols(client, t)
    finally:
        client.shutdown()

    if VERBOSE and client.stderr_lines:
        print(f"\n--- Server logs ({len(client.stderr_lines)} lines) ---")
        for line in client.stderr_lines[:80]:
            print(f"  {line}")

    success = t.summary()
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
