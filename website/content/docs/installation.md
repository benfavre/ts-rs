---
title: Installation
---
# Installation

tsc-rs is distributed as source. You build one binary, `tsc-rs`, with Cargo.

## Requirements

- A stable Rust toolchain with Cargo. Install it from [rustup.rs](https://rustup.rs).
- Git.
- On Linux x86-64 the workspace links with `clang` and `lld` (configured in `.cargo/config.toml`), so install both. On Debian or Ubuntu: `sudo apt install clang lld`.

## Build from source

```bash
git clone https://github.com/benfavre/ts-rs.git
cd ts-rs

# Install the tsc-rs binary into ~/.cargo/bin
cargo install --path crates/tsc_rs_cli
```

Or build in place and use the binary from the target directory:

```bash
cargo build --release
./target/release/tsc-rs --version
```

The release profile uses thin LTO and a single codegen unit, so the first build takes a while. `make build-fast` produces a release binary without LTO when you want a shorter build.

:::note
The repository vendors the TypeScript test corpus under `tests/`, so the clone is large. That corpus is what makes the [conformance numbers](/conformance) reproducible.
:::

## Check the install

```bash
tsc-rs --version
tsc-rs --help
```

## npm

A prebuilt Linux x64 binary is published as `@bext-stack/tsc-rs`:

```bash
npm install --save-dev @bext-stack/tsc-rs
npx tsc-rs --version
```

The npm package follows its own release schedule and can be behind the source tree. See [npm package](/docs/npm) for platform support and the `TSCRS_PATH` override.

## Editor

To use tsc-rs in an editor, build the binary first, then follow [Language server](/docs/language-server) or [VS Code extension](/docs/vscode).

## Next

Continue with the [Quick start](/docs/quick-start).
