//! End-to-end: drive the real `metaarch-lsp` binary over stdio LSP frames —
//! the editor's-eye view of phase 4e. Opens a broken file (positioned
//! diagnostic), fixes it (diagnostics clear), and asks for semantic tokens
//! on `examples/shop.arch` (arch structure + rust/python fragment interiors).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Lsp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Lsp {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_metaarch-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn metaarch-lsp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Lsp {
            child,
            stdin,
            stdout,
            next_id: 0,
        }
    }

    fn send(&mut self, msg: Value) {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let msg = self.recv();
            if msg.get("id").and_then(Value::as_i64) == Some(id) {
                return msg["result"].clone();
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn recv(&mut self) -> Value {
        let mut length = None;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).expect("read header");
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(rest) = line.strip_prefix("Content-Length: ") {
                length = Some(rest.parse::<usize>().unwrap());
            }
        }
        let mut body = vec![0u8; length.expect("Content-Length header")];
        self.stdout.read_exact(&mut body).expect("read body");
        serde_json::from_slice(&body).expect("json body")
    }

    /// Read messages until the next `publishDiagnostics` for `uri`.
    fn diagnostics_for(&mut self, uri: &str) -> Vec<Value> {
        loop {
            let msg = self.recv();
            if msg.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
                && msg["params"]["uri"].as_str() == Some(uri)
            {
                return msg["params"]["diagnostics"].as_array().unwrap().clone();
            }
        }
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn diagnostics_and_semantic_tokens_over_stdio() {
    let mut lsp = Lsp::start();

    let init = lsp.request("initialize", json!({"capabilities": {}}));
    assert!(
        init["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"]
            .as_array()
            .is_some_and(|t| !t.is_empty()),
        "semantic tokens advertised: {init}"
    );
    lsp.notify("initialized", json!({}));

    // A file with a bad port value: the parse error lands on `oops` (line 2,
    // 0-based), covering the word.
    let uri = "file:///demo/broken.arch";
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "arch", "version": 1,
            "text": "system s\nservice x {\n  port oops\n}\n",
        }}),
    );
    let diags = lsp.diagnostics_for(uri);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0]["range"]["start"]["line"], 2);
    assert_eq!(diags[0]["severity"], 1);

    // Fix the file: diagnostics clear on the next change.
    lsp.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{"text": "system s\nservice x {\n  lang rust\n  port 8080\n}\n"}],
        }),
    );
    assert_eq!(lsp.diagnostics_for(uri), Vec::<Value>::new());

    // The shop example: no diagnostics, and semantic tokens cover the arch
    // structure plus both impl fragment interiors.
    let shop = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/shop.arch"
    ))
    .unwrap();
    let shop_uri = "file:///demo/shop.arch";
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": shop_uri, "languageId": "arch", "version": 1, "text": shop,
        }}),
    );
    assert_eq!(lsp.diagnostics_for(shop_uri), Vec::<Value>::new());

    let tokens = lsp.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument": {"uri": shop_uri}}),
    );
    let data = tokens["data"].as_array().unwrap();
    assert!(
        data.len() / 5 > 40,
        "expected a full highlight, got {} tokens",
        data.len() / 5
    );

    lsp.request("shutdown", json!(null));
    lsp.notify("exit", json!(null));
}
