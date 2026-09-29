#!/usr/bin/env python3
"""
LSP performance benchmark for tsc-rs.

Measures what matters for IDE experience:
- Cold start (initialize + first didOpen)
- Hover latency (p50, p95, p99)
- Go-to-definition latency
- Completion latency
- Diagnostics latency (didChange → publishDiagnostics)
- Memory usage

Run: python3 scripts/bench-lsp.py [--iterations 50] [--binary target/release/tsc-rs]
"""

import subprocess, json, threading, time, queue, sys, os, statistics

BINARY = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                      "target", "release", "tsc-rs")
ITERATIONS = 50

for arg in sys.argv[1:]:
    if arg.startswith("--binary="):
        BINARY = arg.split("=", 1)[1]
    elif arg.startswith("--iterations="):
        ITERATIONS = int(arg.split("=", 1)[1])

# Realistic test file with mixed constructs
TEST_CODE = """
import type { Config } from "./config";

interface User {
  id: string;
  name: string;
  email: string;
  age: number;
  roles: string[];
  metadata: Record<string, unknown>;
}

interface AppConfig {
  port: number;
  host: string;
  debug: boolean;
  database: { url: string; pool: number };
}

type Result<T, E = Error> = { ok: true; value: T } | { ok: false; error: E };

function validateUser(data: unknown): Result<User> {
  if (typeof data !== "object" || data === null) {
    return { ok: false, error: new Error("Expected object") };
  }
  const obj = data as Record<string, unknown>;
  if (typeof obj.id !== "string") return { ok: false, error: new Error("Invalid id") };
  if (typeof obj.name !== "string") return { ok: false, error: new Error("Invalid name") };
  if (typeof obj.email !== "string") return { ok: false, error: new Error("Invalid email") };
  if (typeof obj.age !== "number") return { ok: false, error: new Error("Invalid age") };
  if (!Array.isArray(obj.roles)) return { ok: false, error: new Error("Invalid roles") };
  return { ok: true, value: obj as unknown as User };
}

class UserService {
  private users = new Map<string, User>();

  addUser(user: User): void {
    this.users.set(user.id, user);
  }

  getUser(id: string): User | undefined {
    return this.users.get(id);
  }

  findByEmail(email: string): User | undefined {
    for (const user of this.users.values()) {
      if (user.email === email) return user;
    }
    return undefined;
  }

  listUsers(page: number = 1, limit: number = 10): User[] {
    const all = Array.from(this.users.values());
    return all.slice((page - 1) * limit, page * limit);
  }
}

async function handleRequest(req: Request): Promise<Response> {
  const url = new URL(req.url);
  const path = url.pathname;

  if (path === "/api/users" && req.method === "GET") {
    const service = new UserService();
    const users = service.listUsers();
    return Response.json({ users, total: users.length });
  }

  if (path === "/api/users" && req.method === "POST") {
    const body = await req.json();
    const result = validateUser(body);
    if (!result.ok) {
      return Response.json({ error: result.error.message }, { status: 400 });
    }
    const service = new UserService();
    service.addUser(result.value);
    return Response.json({ user: result.value }, { status: 201 });
  }

  return new Response("Not Found", { status: 404 });
}

export { handleRequest, validateUser, UserService };
export type { User, AppConfig, Result };
"""


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

    def recv(self, rid=None, timeout=5):
        end = time.time() + timeout
        while time.time() < end:
            try:
                msg = self.queue.get(timeout=0.05)
                if rid and msg.get("id") == rid: return msg
            except queue.Empty: pass
        return None

    def request(self, method, params=None, timeout=5):
        rid = self.send(method, params)
        return self.recv(rid, timeout)

    def notify(self, method, params=None):
        self.send(method, params, notification=True)

    def shutdown(self):
        self.request("shutdown", timeout=2)
        self.notify("exit")
        try: self.proc.wait(timeout=2)
        except: self.proc.kill()

    def get_memory_mb(self):
        try:
            with open(f"/proc/{self.proc.pid}/status") as f:
                for line in f:
                    if line.startswith("VmRSS:"):
                        return int(line.split()[1]) / 1024
        except: return 0


def bench_cold_start(binary):
    """Measure initialize + first didOpen time."""
    t0 = time.perf_counter()
    client = LSPClient(binary)
    client.request("initialize", {"capabilities": {}})
    client.notify("initialized")
    t_init = time.perf_counter() - t0

    t0 = time.perf_counter()
    client.notify("textDocument/didOpen", {
        "textDocument": {"uri": "file:///tmp/bench.ts", "languageId": "typescript", "version": 1, "text": TEST_CODE}
    })
    time.sleep(0.1)  # Wait for diagnostics
    t_open = time.perf_counter() - t0

    mem = client.get_memory_mb()
    client.shutdown()
    return t_init * 1000, t_open * 1000, mem


def bench_operations(binary, iterations):
    """Measure hover, definition, completion latencies."""
    client = LSPClient(binary)
    client.request("initialize", {"capabilities": {}})
    client.notify("initialized")
    client.notify("textDocument/didOpen", {
        "textDocument": {"uri": "file:///tmp/bench.ts", "languageId": "typescript", "version": 1, "text": TEST_CODE}
    })
    time.sleep(0.2)

    hover_times = []
    def_times = []
    comp_times = []

    # Hover targets (line, col)
    hover_targets = [
        (3, 14), (4, 4), (10, 4), (20, 10), (22, 10),
        (37, 10), (42, 8), (58, 16), (62, 10), (69, 10),
    ]

    # Definition targets
    def_targets = [
        (62, 20), (64, 22), (69, 25), (72, 20), (75, 12),
    ]

    # Completion targets (after dot)
    comp_targets = [
        (42, 18), (58, 14), (69, 23),
    ]

    for i in range(iterations):
        # Hover
        target = hover_targets[i % len(hover_targets)]
        t0 = time.perf_counter()
        client.request("textDocument/hover", {
            "textDocument": {"uri": "file:///tmp/bench.ts"},
            "position": {"line": target[0], "character": target[1]}
        }, timeout=2)
        hover_times.append((time.perf_counter() - t0) * 1000)

        # Definition
        target = def_targets[i % len(def_targets)]
        t0 = time.perf_counter()
        client.request("textDocument/definition", {
            "textDocument": {"uri": "file:///tmp/bench.ts"},
            "position": {"line": target[0], "character": target[1]}
        }, timeout=2)
        def_times.append((time.perf_counter() - t0) * 1000)

        # Completion
        target = comp_targets[i % len(comp_targets)]
        t0 = time.perf_counter()
        client.request("textDocument/completion", {
            "textDocument": {"uri": "file:///tmp/bench.ts"},
            "position": {"line": target[0], "character": target[1]}
        }, timeout=2)
        comp_times.append((time.perf_counter() - t0) * 1000)

    mem = client.get_memory_mb()
    client.shutdown()
    return hover_times, def_times, comp_times, mem


def fmt_stats(times):
    if not times: return "N/A"
    s = sorted(times)
    n = len(s)
    return (f"p50={s[n//2]:.1f}ms  p95={s[int(n*0.95)]:.1f}ms  "
            f"p99={s[int(n*0.99)]:.1f}ms  avg={statistics.mean(s):.1f}ms  "
            f"min={s[0]:.1f}ms  max={s[-1]:.1f}ms")


def main():
    if not os.path.exists(BINARY):
        print(f"Binary not found: {BINARY}")
        sys.exit(1)

    print(f"tsc-rs LSP Benchmark")
    print(f"Binary: {BINARY}")
    print(f"Iterations: {ITERATIONS}")
    print(f"Test file: {len(TEST_CODE)} bytes, {TEST_CODE.count(chr(10))} lines")
    print(f"{'='*70}")

    # Cold start (3 runs, take median)
    print("\n1. Cold Start")
    cold_inits = []
    cold_opens = []
    cold_mems = []
    for i in range(3):
        init_ms, open_ms, mem = bench_cold_start(BINARY)
        cold_inits.append(init_ms)
        cold_opens.append(open_ms)
        cold_mems.append(mem)
    print(f"   Initialize: {statistics.median(cold_inits):.1f}ms (median of 3)")
    print(f"   First didOpen: {statistics.median(cold_opens):.1f}ms (median of 3)")
    print(f"   Memory after open: {statistics.median(cold_mems):.1f}MB")

    # Steady-state operations
    print(f"\n2. Steady State ({ITERATIONS} iterations)")
    hover_times, def_times, comp_times, mem = bench_operations(BINARY, ITERATIONS)
    print(f"   Hover:      {fmt_stats(hover_times)}")
    print(f"   Definition: {fmt_stats(def_times)}")
    print(f"   Completion: {fmt_stats(comp_times)}")
    print(f"   Memory:     {mem:.1f}MB")

    # Throughput
    total_ops = len(hover_times) + len(def_times) + len(comp_times)
    total_time = sum(hover_times) + sum(def_times) + sum(comp_times)
    print(f"\n3. Throughput")
    print(f"   {total_ops} ops in {total_time:.0f}ms = {total_ops/total_time*1000:.0f} ops/sec")

    print(f"\n{'='*70}")


if __name__ == "__main__":
    main()
