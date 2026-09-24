#!/usr/bin/env python3
"""Open Nia files in a real LSP session and print the server's diagnostics."""

import argparse
import asyncio
import json
from pathlib import Path
import sys


async def send(process, message):
    body = json.dumps({"jsonrpc": "2.0", **message}, ensure_ascii=False).encode("utf-8")
    process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body)
    await process.stdin.drain()


async def receive(process):
    async def read_message():
        length = None
        while True:
            header = await process.stdout.readline()
            if not header:
                raise RuntimeError("Server closed stdout before replying")
            if header == b"\r\n":
                break
            name, value = header.decode("ascii").split(":", 1)
            if name.lower() == "content-length":
                length = int(value.strip())
        if length is None:
            raise RuntimeError("Server reply has no Content-Length")
        message = json.loads(await process.stdout.readexactly(length))
        if "error" in message:
            raise RuntimeError(f"LSP error: {message['error']}")
        return message

    return await asyncio.wait_for(read_message(), timeout=10)


async def diagnostics(process, uri):
    message = await receive(process)
    if (message.get("method") != "textDocument/publishDiagnostics"
            or message.get("params", {}).get("uri") != uri):
        raise RuntimeError(f"Expected diagnostics for {uri}, received {message}")
    return message["params"]["diagnostics"]


async def check(server, files):
    documents = [(path.resolve(), path.read_text(encoding="utf-8")) for path in files]
    process = await asyncio.create_subprocess_exec(
        str(server.resolve()), stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE
    )
    found_errors = False
    try:
        await send(process, {
            "id": 1, "method": "initialize",
            "params": {"processId": None, "rootUri": None, "capabilities": {}},
        })
        response = await receive(process)
        if response.get("id") != 1 or "result" not in response:
            raise RuntimeError(f"Unexpected initialize response: {response}")
        await send(process, {"method": "initialized", "params": {}})

        for path, source in documents:
            uri = path.as_uri()
            await send(process, {
                "method": "textDocument/didOpen",
                "params": {"textDocument": {
                    "uri": uri, "languageId": "nia", "version": 1, "text": source,
                }},
            })
            errors = await diagnostics(process, uri)
            if not errors:
                print(f"{path}: OK (no diagnostics)")
            for diagnostic in errors:
                start = diagnostic["range"]["start"]
                severity = diagnostic.get("severity", 1)
                label = {1: "error", 2: "warning", 3: "info", 4: "hint"}.get(severity, "diagnostic")
                print(f"{path}:{start['line'] + 1}:{start['character'] + 1}: "
                      f"{label}: {diagnostic['message']}")
                found_errors |= severity == 1

            await send(process, {
                "method": "textDocument/didClose", "params": {"textDocument": {"uri": uri}},
            })
            if await diagnostics(process, uri):
                raise RuntimeError("Server did not clear diagnostics after didClose")

        await send(process, {"id": 2, "method": "shutdown", "params": None})
        response = await receive(process)
        if response != {"jsonrpc": "2.0", "id": 2, "result": None}:
            raise RuntimeError(f"Unexpected shutdown response: {response}")
        await send(process, {"method": "exit"})
        process.stdin.close()
        status = await asyncio.wait_for(process.wait(), timeout=10)
        if status != 0:
            raise RuntimeError(f"Server exited with status {status}")
        return int(found_errors)
    finally:
        if process.returncode is None:
            process.kill()
            await process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="+", type=Path, help="Nia source files to check")
    parser.add_argument(
        "--server", type=Path,
        default=Path(__file__).resolve().parents[1] / "target" / "debug" / "nia-lsp",
        help="server binary (default: nia-lsp/target/debug/nia-lsp; run cargo build first)",
    )
    args = parser.parse_args()
    try:
        return asyncio.run(check(args.server, args.files))
    except (OSError, RuntimeError, ValueError, asyncio.TimeoutError, asyncio.IncompleteReadError) as error:
        print(f"check.py: {error or type(error).__name__}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
