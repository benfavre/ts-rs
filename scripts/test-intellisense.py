#!/usr/bin/env python3
"""
Intellisense test harness for tsc-rs.

Tests completions, signature help, and code actions.
Run: python3 scripts/test-intellisense.py [--verbose]
"""

import subprocess, json, threading, time, queue, sys, os

BINARY = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                      "target", "release", "tsc-rs")
VERBOSE = "--verbose" in sys.argv or "-v" in sys.argv


class LSPClient:
    def __init__(self, binary):
        self.proc = subprocess.Popen(
            [binary, "--lsp"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            cwd="/tmp"
        )
        self.queue = queue.Queue()
        self._next_id = 1
        self._reader = threading.Thread(target=self._read, daemon=True)
        self._reader.start()

    def _read(self):
        buf = b""
        try:
            while True:
                chunk = self.proc.stdout.read(1)
                if not chunk: break
                buf += chunk
                while b"Content-Length:" in buf:
                    try:
                        idx = buf.index(b"Content-Length:")
                        nl = buf.index(b"\r\n\r\n", idx)
                        length = int(buf[idx:nl].split(b":")[1].strip())
                        start = nl + 4
                        if len(buf) >= start + length:
                            self.queue.put(json.loads(buf[start:start+length]))
                            buf = buf[start+length:]
                        else: break
                    except: break
        except: pass

    def send(self, method, params=None, notification=False):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None: msg["params"] = params
        if not notification:
            msg["id"] = self._next_id
            self._next_id += 1
        body = json.dumps(msg).encode()
        self.proc.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.proc.stdin.flush()
        return msg.get("id")

    def recv(self, rid=None, timeout=3):
        end = time.time() + timeout
        while time.time() < end:
            try:
                msg = self.queue.get(timeout=0.05)
                if rid and msg.get("id") == rid: return msg
            except queue.Empty: pass
        return None

    def request(self, method, params=None, timeout=3):
        rid = self.send(method, params)
        return self.recv(rid, timeout)

    def notify(self, method, params=None):
        self.send(method, params, notification=True)

    def shutdown(self):
        self.request("shutdown", timeout=2)
        self.notify("exit")
        try: self.proc.wait(timeout=2)
        except: self.proc.kill()

    def open_file(self, uri, text):
        self.notify("textDocument/didOpen", {
            "textDocument": {"uri": uri, "languageId": "typescript", "version": 1, "text": text}
        })
        time.sleep(0.15)

    def completions(self, uri, line, char):
        resp = self.request("textDocument/completion", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": char}
        })
        result = resp.get("result") if resp else None
        if result is None: return []
        if isinstance(result, list): return result
        if isinstance(result, dict): return result.get("items", [])
        return []

    def completion_labels(self, uri, line, char):
        items = self.completions(uri, line, char)
        return [i.get("label", "") for i in items]

    def signature_help(self, uri, line, char):
        resp = self.request("textDocument/signatureHelp", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": char}
        })
        return resp.get("result") if resp else None


class IntelliRunner:
    def __init__(self):
        self.passed = 0
        self.failed = 0
        self.errors = []

    def check(self, name, condition, actual=""):
        if condition:
            self.passed += 1
            if VERBOSE: print(f"    PASS {name}")
        else:
            self.failed += 1
            self.errors.append(name)
            print(f"    FAIL {name}")
            if actual: print(f"      got: {actual}")

    def summary(self):
        total = self.passed + self.failed
        print(f"\n{'='*70}")
        print(f"Intellisense: {self.passed}/{total} ({100*self.passed//total if total else 0}%)")
        if self.errors:
            print(f"Failures:")
            for e in self.errors:
                print(f"  - {e}")
        print(f"{'='*70}")
        return self.failed == 0


def test_dot_completions(client, t):
    """Completions after dot on various types."""
    print("\n  1. Dot completions")
    code = """const msg = "hello";
const nums = [1, 2, 3];
const obj = { name: "Alice", age: 30 };
const map = new Map<string, number>();
msg.
nums.
obj.
map.
"""
    uri = "file:///tmp/intel-dot.ts"
    client.open_file(uri, code)

    # String methods (msg. is on line 4, dot at col 3, cursor at col 4)
    labels = client.completion_labels(uri, 4, 4)
    t.check("string.length", "length" in labels, str(labels[:10]))
    t.check("string.toUpperCase", "toUpperCase" in labels, str(labels[:10]))
    t.check("string.split", "split" in labels, str(labels[:10]))
    t.check("string.trim", "trim" in labels, str(labels[:10]))

    # Array methods (nums. is on line 5)
    labels = client.completion_labels(uri, 5, 5)
    t.check("array.map", "map" in labels, str(labels[:10]))
    t.check("array.filter", "filter" in labels, str(labels[:10]))
    t.check("array.push", "push" in labels, str(labels[:10]))
    t.check("array.length", "length" in labels, str(labels[:10]))

    # Object properties (obj. is on line 6)
    labels = client.completion_labels(uri, 6, 4)
    t.check("obj.name", "name" in labels, str(labels[:10]))
    t.check("obj.age", "age" in labels, str(labels[:10]))

    # Map methods (map. is on line 7)
    labels = client.completion_labels(uri, 7, 4)
    t.check("map.get", "get" in labels, str(labels[:10]))
    t.check("map.set", "set" in labels, str(labels[:10]))
    t.check("map.has", "has" in labels, str(labels[:10]))


def test_scope_completions(client, t):
    """Completions for in-scope variables."""
    print("\n  2. Scope completions")
    code = """const greeting = "hello";
function sayHello(name: string) {
  const message = greeting + " " + name;

}
"""
    uri = "file:///tmp/intel-scope.ts"
    client.open_file(uri, code)

    # Inside function body — should see local + outer scope
    labels = client.completion_labels(uri, 3, 2)
    t.check("scope: greeting visible", "greeting" in labels, str(labels[:15]))
    t.check("scope: name visible", "name" in labels, str(labels[:15]))
    t.check("scope: message visible", "message" in labels, str(labels[:15]))
    t.check("scope: sayHello visible", "sayHello" in labels, str(labels[:15]))


def test_signature_help(client, t):
    """Signature help for function calls."""
    print("\n  3. Signature help")
    code = """function greet(name: string, age: number): string {
  return name + " " + age;
}
greet(
"""
    uri = "file:///tmp/intel-sig.ts"
    client.open_file(uri, code)

    sig = client.signature_help(uri, 3, 6)
    t.check("signature help exists", sig is not None, str(sig))
    if sig:
        sigs = sig.get("signatures", [])
        t.check("has signature", len(sigs) > 0, str(sigs))
        if sigs:
            label = sigs[0].get("label", "")
            t.check("sig has name", "greet" in label, label)
            t.check("sig has params", "name" in label and "age" in label, label)


def test_completion_kinds(client, t):
    """Completion items have correct kinds."""
    print("\n  4. Completion item kinds")
    code = """function myFunc() {}
class MyClass {}
const myConst = 42;
interface MyInterface { x: number; }
enum MyEnum { A, B }

"""
    uri = "file:///tmp/intel-kinds.ts"
    client.open_file(uri, code)

    items = client.completions(uri, 6, 0)
    items_by_label = {i["label"]: i for i in items}

    if "myFunc" in items_by_label:
        kind = items_by_label["myFunc"].get("kind")
        t.check("function kind=3", kind == 3, f"kind={kind}")
    else:
        t.check("function in completions", False, str([i["label"] for i in items[:10]]))

    if "MyClass" in items_by_label:
        kind = items_by_label["MyClass"].get("kind")
        t.check("class kind=7", kind == 7, f"kind={kind}")
    else:
        t.check("class in completions", False)

    if "myConst" in items_by_label:
        kind = items_by_label["myConst"].get("kind")
        t.check("variable kind=6", kind == 6, f"kind={kind}")
    else:
        t.check("variable in completions", False)


def test_no_completions_in_strings(client, t):
    """No identifier completions inside string literals."""
    print("\n  5. No completions in strings")
    code = 'const x = "hello world ";\nconst y = x;\n'
    uri = "file:///tmp/intel-nostr.ts"
    client.open_file(uri, code)

    labels = client.completion_labels(uri, 0, 16)  # Inside "hello world"
    # Should have very few or no identifier completions inside string
    has_identifiers = any(l in labels for l in ["const", "function", "x", "y"])
    t.check("no identifiers in string", not has_identifiers, str(labels[:5]))


def main():
    if not os.path.exists(BINARY):
        print(f"Binary not found: {BINARY}")
        sys.exit(1)

    print(f"tsc-rs Intellisense Tests")
    print(f"Binary: {BINARY}")

    t = IntelliRunner()
    client = LSPClient(BINARY)

    resp = client.request("initialize", {"capabilities": {}})
    assert resp and resp.get("result"), "Failed to initialize"
    client.notify("initialized")
    time.sleep(0.1)

    try:
        test_dot_completions(client, t)
        test_scope_completions(client, t)
        test_signature_help(client, t)
        test_completion_kinds(client, t)
        test_no_completions_in_strings(client, t)
    finally:
        client.shutdown()

    success = t.summary()
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
