# nia-lsp

An initial LSP server for the Nia language, built with Rust and
[tower-lsp-server](https://docs.rs/tower-lsp-server/0.23.0/).
It uses the parser and type checker from the neighboring `nialang` repository.

## Features

- LSP communication over stdin/stdout.
- Session initialization and termination (`initialize`, `shutdown`, `exit`).
- In-memory storage of open documents with full text synchronization
  (`didOpen`, `didChange`, `didClose`).
- Diagnostics published when documents are opened or changed, and cleared when closed.
- Syntax, declaration, and type checking without generating LLVM IR or running code.
- Analysis of unsaved changes, with the document version included in diagnostics.

## Build and run

Requires Rust with edition 2024 support. The repositories must be siblings:

```text
nia/
├── nialang/
└── nia-lsp/
```

From `nia-lsp`:

```sh
cargo build --release
./target/release/nia-lsp
```

The server waits for LSP messages from an editor. No output is expected when
started manually. stdout is reserved for the protocol; use stderr for any future logging.
The server does not require `clang` or `qir-runner`.

Configure your editor's LSP client with:

- Command: the absolute path to `nia-lsp/target/release/nia-lsp`.
- Arguments: an empty list.
- Transport: stdio.
- Language ID: `nia`; file extension: `.nia`.

This repository does not yet include an editor plugin.

## Check examples without an editor

The `scripts/check.py` script starts the actual server and sends LSP messages
to initialize the session, open and close files, and terminate the session.
It prints the diagnostics it receives. Requires Python 3.9+ with no extra packages.

From `nia-lsp`:

```sh
cargo build --locked
python3 scripts/check.py examples/valid.nia
python3 scripts/check.py examples/type_error.nia examples/syntax_error.nia
python3 scripts/check.py ../nialang/examples/sample_floats.nia
```

For a valid file, the script prints `OK (no diagnostics)`. For a file with errors,
it prints the path, line, column, and server message. Lines and columns in the
output are numbered from one: position `1:1` for parser and type errors reflects
the compiler's current source-location limitation.

The script checks the code without executing the Nia program.
Exit codes: `0` means no errors, `1` means the server reported errors in the Nia code,
and `2` means a startup or communication error. The files `type_error.nia` and
`syntax_error.nia` contain intentional errors.

You can pass any `.nia` file or multiple files. If the server binary is located
elsewhere, use `--server /path/to/nia-lsp`.

To verify that an error is reported and then cleared after a text change
within the same LSP session, run the existing integration test:

```sh
cargo test --locked --test stdio -- --nocapture
```

## Project structure

```text
src/main.rs      — stdio transport startup
src/server.rs    — LSP handlers and open documents
src/analysis.rs  — compiler diagnostics adapter
tests/stdio.rs   — full session test using an actual server process
scripts/check.py — manual checking of .nia files over LSP
examples/        — valid code and intentional errors for demonstration
```

## Initial limitations

The `nialang` parser and type checker return errors without source ranges,
so their diagnostics point to the start of the file (LSP position `0:0`).
The adapter's lexical checks for unsupported characters and unterminated strings
report ranges in UTF-16. The next step is to add source ranges to the compiler API
and use them here.

Each open document is analyzed separately. Inline modules declared with
`mod name { ... }` are supported; loading modules from other files via `mod name;`
and project-wide analysis are not yet implemented. A syntax or declaration error
stops analysis of the file. If those checks pass, type checking reports the first
error in each function.

Full synchronization is used: the editor sends the entire text on each change.
Messages are processed sequentially to preserve the order of updates and diagnostics.
Background analysis, request cancellation, completion, hover, go to definition,
and formatting are future extensions.

## Checks

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --no-deps -- -D warnings
```
