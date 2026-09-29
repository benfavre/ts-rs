#!/usr/bin/env python3
"""
Gap-focused test suite for tsc-rs.

Tests organized by category of known weakness. Each test documents what
should work and whether it currently does. Run to see a clear picture
of what to fix next.

    python3 scripts/test-gaps.py [--verbose]
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

    def hover(self, uri, line, char):
        resp = self.request("textDocument/hover", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": char}
        })
        if resp and resp.get("result") and resp["result"].get("contents"):
            val = resp["result"]["contents"].get("value", "")
            return val.replace("```typescript\n", "").replace("\n```", "").split("\n---")[0].strip()
        return None

    def definition(self, uri, line, char):
        resp = self.request("textDocument/definition", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": char}
        })
        r = resp.get("result") if resp else None
        if r is None: return None
        return [r] if isinstance(r, dict) else r


class GapRunner:
    def __init__(self):
        self.categories = {}  # category -> [(name, passed, actual, expected)]

    def check(self, category, name, actual, expected_substr):
        if category not in self.categories:
            self.categories[category] = []
        passed = actual is not None and expected_substr.lower() in actual.lower()
        self.categories[category].append((name, passed, actual, expected_substr))
        if VERBOSE:
            status = "PASS" if passed else "FAIL"
            print(f"    {status} {name}: {(actual or '(null)')[:70]}")

    def check_not_none(self, category, name, actual):
        if category not in self.categories:
            self.categories[category] = []
        passed = actual is not None
        self.categories[category].append((name, passed, actual, "not null"))
        if VERBOSE:
            status = "PASS" if passed else "FAIL"
            print(f"    {status} {name}: {(actual or '(null)')[:70]}")

    def summary(self):
        total_pass = total_fail = 0
        print(f"\n{'='*80}")
        print(f"GAP ANALYSIS RESULTS")
        print(f"{'='*80}")
        for cat, tests in sorted(self.categories.items()):
            p = sum(1 for _, ok, _, _ in tests if ok)
            f = len(tests) - p
            total_pass += p
            total_fail += f
            pct = 100 * p // len(tests) if tests else 0
            bar = "=" * (pct // 5) + "-" * (20 - pct // 5)
            status = "OK" if f == 0 else "GAPS"
            print(f"\n  {cat}")
            print(f"    [{bar}] {p}/{len(tests)} ({pct}%) {status}")
            for name, ok, actual, expected in tests:
                if not ok:
                    print(f"      FAIL {name}: got {(actual or '(null)')[:50]}, want '{expected}'")

        total = total_pass + total_fail
        print(f"\n{'='*80}")
        print(f"TOTAL: {total_pass}/{total} ({100*total_pass//total if total else 0}%)")
        print(f"{'='*80}")
        return total_fail == 0


def test_generic_inference(client, g):
    """Generic type parameter inference through callbacks and methods."""
    cat = "1. Generic inference"
    print(f"\n  {cat}")
    code = """const nums = [1, 2, 3];
const mapped = nums.map(n => n * 2);
const filtered = nums.filter(n => n > 1);
const reduced = nums.reduce((acc, n) => acc + n, 0);
const found = nums.find(n => n > 1);
const strs = nums.map(n => String(n));
const promise = Promise.resolve(42);
const arr = Array.from([1, 2, 3]);
"""
    uri = "file:///tmp/gap-generics.ts"
    client.open_file(uri, code)

    g.check(cat, "map returns number[]", client.hover(uri, 1, 6), "number[]")
    g.check(cat, "filter returns number[]", client.hover(uri, 2, 6), "number[]")
    g.check(cat, "reduce returns number", client.hover(uri, 3, 6), "number")
    g.check(cat, "find returns number | undefined", client.hover(uri, 4, 6), "number")
    g.check(cat, "map to string[] ", client.hover(uri, 5, 6), "string[]")
    g.check(cat, "Promise.resolve", client.hover(uri, 6, 6), "Promise")
    g.check(cat, "Array.from", client.hover(uri, 7, 6), "number[]")


def test_member_chain(client, g):
    """Multi-level member access chains."""
    cat = "2. Member access chains"
    print(f"\n  {cat}")
    code = """const obj = { a: { b: { c: 42 } } };
const c = obj.a.b.c;
const msg = "hello world";
const parts = msg.split(" ");
const firstLen = msg.split(" ")[0].length;
const trimmed = msg.trim().toUpperCase();
const json = JSON.stringify({ x: 1 });
const parsed = JSON.parse(json);
const env = process.env.NODE_ENV;
"""
    uri = "file:///tmp/gap-chains.ts"
    client.open_file(uri, code)

    g.check(cat, "deep access obj.a.b.c", client.hover(uri, 1, 6), "number")
    g.check(cat, "split returns string[]", client.hover(uri, 3, 6), "string[]")
    g.check(cat, "chained trim().toUpperCase()", client.hover(uri, 5, 6), "string")
    g.check(cat, "JSON.stringify", client.hover(uri, 6, 6), "string")
    g.check(cat, "JSON.parse", client.hover(uri, 7, 6), "any")
    g.check(cat, "process.env.NODE_ENV", client.hover(uri, 8, 6), "string")


def test_narrowing(client, g):
    """Type narrowing through control flow."""
    cat = "3. Type narrowing"
    print(f"\n  {cat}")
    code = """function test(x: string | number) {
  if (typeof x === "string") {
    const upper = x.toUpperCase();
    return upper;
  }
  const fixed = x.toFixed(2);
  return fixed;
}
function testNull(x: string | null) {
  if (x === null) return "default";
  const len = x.length;
  return len;
}
function testTruthy(x: string | undefined) {
  if (x) {
    const upper = x.toUpperCase();
  }
}
"""
    uri = "file:///tmp/gap-narrow.ts"
    client.open_file(uri, code)

    g.check(cat, "typeof narrows to string", client.hover(uri, 2, 10), "string")
    g.check(cat, "typeof narrows to number", client.hover(uri, 5, 8), "string")
    g.check(cat, "null check narrows", client.hover(uri, 10, 8), "number")


def test_class_features(client, g):
    """Class features: inheritance, generics, static members."""
    cat = "4. Class features"
    print(f"\n  {cat}")
    code = """class Animal {
  constructor(public name: string) {}
  speak(): string { return this.name; }
}
class Dog extends Animal {
  bark(): string { return "woof"; }
}
const dog = new Dog("Rex");
const dogName = dog.name;
const dogBark = dog.bark();
const dogSpeak = dog.speak();

class Container<T> {
  constructor(private value: T) {}
  get(): T { return this.value; }
}
const box = new Container<number>(42);
const val = box.get();

class MathUtils {
  static add(a: number, b: number): number { return a + b; }
  static PI = 3.14159;
}
const sum = MathUtils.add(1, 2);
const pi = MathUtils.PI;
"""
    uri = "file:///tmp/gap-class.ts"
    client.open_file(uri, code)

    g.check(cat, "new Dog() is Dog", client.hover(uri, 7, 6), "Dog")
    g.check(cat, "dog.name inherited", client.hover(uri, 8, 6), "string")
    g.check(cat, "dog.bark()", client.hover(uri, 9, 6), "string")
    g.check(cat, "dog.speak() inherited", client.hover(uri, 10, 6), "string")
    g.check(cat, "generic Container<number>", client.hover(uri, 16, 6), "Container")
    g.check(cat, "box.get() returns number", client.hover(uri, 17, 6), "number")
    g.check(cat, "static method", client.hover(uri, 23, 6), "number")
    g.check(cat, "static property", client.hover(uri, 24, 6), "number")


def test_async_patterns(client, g):
    """Async/await, Promise chains, try/catch."""
    cat = "5. Async patterns"
    print(f"\n  {cat}")
    code = """async function fetchUser(): Promise<{ name: string; age: number }> {
  return { name: "Alice", age: 30 };
}
const user = await fetchUser();
const userName = user.name;
const userAge = user.age;

async function fetchText(url: string): Promise<string> {
  const response = await fetch(url);
  const text = await response.text();
  return text;
}
"""
    uri = "file:///tmp/gap-async.ts"
    client.open_file(uri, code)

    g.check(cat, "await unwraps Promise", client.hover(uri, 3, 6), "name:")
    g.check(cat, "member of awaited result", client.hover(uri, 4, 6), "string")
    g.check(cat, "await fetch gives Response", client.hover(uri, 8, 8), "Response")
    g.check(cat, "await response.text()", client.hover(uri, 9, 8), "string")


def test_union_intersection(client, g):
    """Union and intersection types."""
    cat = "6. Union & intersection"
    print(f"\n  {cat}")
    code = """type StringOrNumber = string | number;
const x: StringOrNumber = "hello";
type A = { a: number };
type B = { b: string };
type AB = A & B;
const ab: AB = { a: 1, b: "x" };
function acceptUnion(x: string | number | boolean): void {}
type Result<T> = { ok: true; value: T } | { ok: false; error: string };
const r: Result<number> = { ok: true, value: 42 };
"""
    uri = "file:///tmp/gap-union.ts"
    client.open_file(uri, code)

    g.check(cat, "union type alias", client.hover(uri, 0, 5), "StringOrNumber")
    g.check(cat, "var with union type", client.hover(uri, 1, 6), "StringOrNumber")
    g.check(cat, "intersection AB", client.hover(uri, 4, 5), "AB")
    g.check(cat, "var with intersection", client.hover(uri, 5, 6), "AB")
    g.check(cat, "Result generic union", client.hover(uri, 8, 6), "Result")


def test_imports_cross_file(client, g):
    """Cross-file import resolution and hover."""
    cat = "7. Cross-file imports"
    print(f"\n  {cat}")
    mod_code = """export interface Config { port: number; host: string; }
export function createServer(config: Config): { start: () => void } {
  return { start: () => {} };
}
export const VERSION = "1.0.0";
export type ID = string | number;
"""
    # Write module to disk so resolver can find it
    import os
    with open("/tmp/gap-mod.ts", "w") as f:
        f.write(mod_code)
    client.open_file("file:///tmp/gap-mod.ts", mod_code)

    code = """import { Config, createServer, VERSION, ID } from "./gap-mod";
const cfg: Config = { port: 3000, host: "localhost" };
const server = createServer(cfg);
const v = VERSION;
type MyID = ID;
"""
    uri = "file:///tmp/gap-import.ts"
    client.open_file(uri, code)

    g.check_not_none(cat, "import Config hover", client.hover(uri, 0, 10))
    g.check(cat, "cfg has Config type", client.hover(uri, 1, 6), "Config")
    g.check(cat, "createServer return type", client.hover(uri, 2, 6), "start:")
    g.check(cat, "VERSION is string", client.hover(uri, 3, 6), "string")
    g.check_not_none(cat, "type alias import", client.hover(uri, 4, 5))


def test_definition_targets(client, g):
    """Go-to-definition accuracy."""
    cat = "8. Definition targets"
    print(f"\n  {cat}")
    code = """const greeting = "hello";
function say(msg: string): string { return msg; }
const result = say(greeting);
interface User { name: string; }
const user: User = { name: "Alice" };
"""
    uri = "file:///tmp/gap-def.ts"
    client.open_file(uri, code)

    d = client.definition(uri, 2, 15)
    g.check_not_none(cat, "def of say() call", d)
    if d:
        g.check(cat, "say() points to line 1", str(d[0].get("range", {}).get("start", {}).get("line", -1)), "1")

    d = client.definition(uri, 2, 19)
    g.check_not_none(cat, "def of greeting ref", d)
    if d:
        g.check(cat, "greeting points to line 0", str(d[0].get("range", {}).get("start", {}).get("line", -1)), "0")


def test_template_literals(client, g):
    """Template literal types and expressions."""
    cat = "9. Template literals"
    print(f"\n  {cat}")
    code = 'const name = "world";\nconst greeting = `hello ${name}`;\nconst multi = `line1\nline2`;\n'
    uri = "file:///tmp/gap-template.ts"
    client.open_file(uri, code)

    g.check(cat, "template literal is string", client.hover(uri, 1, 6), "string")
    g.check(cat, "multiline template", client.hover(uri, 2, 6), "string")


def test_optional_chaining(client, g):
    """Optional chaining and nullish coalescing."""
    cat = "10. Optional chaining"
    print(f"\n  {cat}")
    code = """interface Config { db?: { host: string; port: number } }
const cfg: Config = {};
const host = cfg.db?.host;
const port = cfg.db?.port ?? 5432;
const len = cfg.db?.host?.length;
"""
    uri = "file:///tmp/gap-optional.ts"
    client.open_file(uri, code)

    g.check(cat, "optional chain result", client.hover(uri, 2, 6), "string")
    g.check(cat, "nullish coalescing", client.hover(uri, 3, 6), "number")


def main():
    if not os.path.exists(BINARY):
        print(f"Binary not found: {BINARY}")
        sys.exit(1)

    print(f"tsc-rs Gap Analysis")
    print(f"Binary: {BINARY}")

    g = GapRunner()
    client = LSPClient(BINARY)

    resp = client.request("initialize", {"capabilities": {}})
    assert resp and resp.get("result"), "Failed to initialize"
    client.notify("initialized")
    time.sleep(0.1)

    try:
        test_generic_inference(client, g)
        test_member_chain(client, g)
        test_narrowing(client, g)
        test_class_features(client, g)
        test_async_patterns(client, g)
        test_union_intersection(client, g)
        test_imports_cross_file(client, g)
        test_definition_targets(client, g)
        test_template_literals(client, g)
        test_optional_chaining(client, g)
    finally:
        client.shutdown()

    success = g.summary()
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
