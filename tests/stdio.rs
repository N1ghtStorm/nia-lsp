use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

struct Client {
    process: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Client {
    fn start() -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_nia-lsp"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            input: process.stdin.take().unwrap(),
            output: BufReader::new(process.stdout.take().unwrap()),
            process,
        }
    }

    async fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.input.write_all(header.as_bytes()).await.unwrap();
        self.input.write_all(&body).await.unwrap();
        self.input.flush().await.unwrap();
    }

    async fn receive(&mut self) -> Value {
        timeout(Duration::from_secs(5), async {
            let mut length = None;
            loop {
                let mut header = String::new();
                assert_ne!(self.output.read_line(&mut header).await.unwrap(), 0);
                if header == "\r\n" {
                    break;
                }
                if let Some(value) = header.strip_prefix("Content-Length:") {
                    length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let mut body = vec![0; length.expect("missing Content-Length")];
            self.output.read_exact(&mut body).await.unwrap();
            serde_json::from_slice(&body).unwrap()
        })
        .await
        .expect("server did not respond within five seconds")
    }
}

#[tokio::test(flavor = "current_thread")]
async fn document_lifecycle_over_stdio() {
    let mut client = Client::start();
    client
        .send(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "processId": null, "rootUri": null, "capabilities": {} }
        }))
        .await;
    let response = client.receive().await;
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["serverInfo"]["name"], "nia-lsp");
    assert_eq!(
        response["result"]["capabilities"]["textDocumentSync"]["change"],
        1
    );
    client
        .send(json!({
            "jsonrpc": "2.0", "method": "initialized", "params": {}
        }))
        .await;

    // These files deliberately do not exist: analysis must use editor buffers.
    let uri = "file:///nia-lsp-test/main.nia";
    let second_uri = "file:///nia-lsp-test/other.nia";
    for document_uri in [uri, second_uri] {
        client
            .send(json!({
                "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": document_uri, "languageId": "nia", "version": 1,
                    "text": "fn main() i32 { true }"
                } }
            }))
            .await;
        let diagnostics = client.receive().await;
        assert_eq!(diagnostics["method"], "textDocument/publishDiagnostics");
        assert_eq!(diagnostics["params"]["uri"], document_uri);
        assert_eq!(diagnostics["params"]["version"], 1);
        assert_eq!(diagnostics["params"]["diagnostics"][0]["severity"], 1);
        assert!(
            diagnostics["params"]["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains("type error")
        );
    }

    client
        .send(json!({
            "jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{ "text": "fn main() i32 { println(\"Привет 🌍\"); 0 }" }]
            }
        }))
        .await;
    let diagnostics = client.receive().await;
    assert_eq!(diagnostics["params"]["uri"], uri);
    assert_eq!(diagnostics["params"]["version"], 2);
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));

    // A stale update must not publish old errors or overwrite the new buffer.
    client
        .send(json!({
            "jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 1 },
                "contentChanges": [{ "text": "fn broken(" }]
            }
        }))
        .await;
    client
        .send(json!({
            "jsonrpc": "2.0", "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": second_uri } }
        }))
        .await;
    let diagnostics = client.receive().await;
    assert_eq!(diagnostics["params"]["uri"], second_uri);
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));

    client
        .send(json!({
            "jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null
        }))
        .await;
    assert_eq!(
        client.receive().await,
        json!({ "jsonrpc": "2.0", "id": 2, "result": null })
    );
    client
        .send(json!({ "jsonrpc": "2.0", "method": "exit" }))
        .await;
    // Editors need exit to work even while the input pipe is still open.
    let status = timeout(Duration::from_secs(5), client.process.wait())
        .await
        .expect("server did not exit")
        .unwrap();
    assert!(status.success());
}
