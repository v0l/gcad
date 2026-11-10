use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Client {
    child: Child,
    stdin: ChildStdin,
    messages: mpsc::Receiver<Value>,
    next: u64,
}

impl Client {
    fn start() -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gcad"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawns gcad lsp");
        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    if stdout.read_line(&mut header).unwrap_or(0) == 0 {
                        return;
                    }
                    let header = header.trim();
                    if header.is_empty() {
                        break;
                    }
                    if let Some(n) = header.strip_prefix("Content-Length: ") {
                        length = n.parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                if stdout.read_exact(&mut body).is_err() {
                    return;
                }
                if sender.send(serde_json::from_slice(&body).unwrap()).is_err() {
                    return;
                }
            }
        });
        let mut client = Client {
            child,
            stdin,
            messages,
            next: 0,
        };
        client.request("initialize", json!({"capabilities": {}}));
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, value: Value) {
        let body = value.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.wait(Duration::from_secs(60), |m| m["id"] == json!(id));
            return message["result"].clone();
        }
    }

    fn wait(&mut self, limit: Duration, accept: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + limit;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let message = self.messages.recv_timeout(left).expect("a message in time");
            if accept(&message) {
                return message;
            }
        }
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "gcad", "version": 1, "text": text}}),
        );
    }

    fn diagnostics(&mut self, uri: &str) -> Vec<Value> {
        let message = self.wait(Duration::from_secs(60), |m| {
            m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri
        });
        message["params"]["diagnostics"].as_array().unwrap().clone()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn uri(path: &str) -> String {
    format!("file://{}/{path}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn lsp_bad_op_publishes_one_error_on_its_line() {
    let mut client = Client::start();
    let file = uri("target/lsp-bad.gcad");
    client.open(&file, "let w=10\nrect w w\nnonsense 1 2\nextrude 2\n");
    let found = client.diagnostics(&file);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["range"]["start"]["line"], 2);
    assert_eq!(found[0]["severity"], 1);
    assert!(
        found[0]["message"].as_str().unwrap().contains("nonsense"),
        "{found:?}"
    );
}

#[test]
fn lsp_fixing_the_line_clears_the_error() {
    let mut client = Client::start();
    let file = uri("target/lsp-fix.gcad");
    client.open(&file, "let w=10\nrect w w\nnonsense 1 2\n");
    assert_eq!(client.diagnostics(&file).len(), 1);
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": file, "version": 2},
            "contentChanges": [{"text": "let w=10\nrect w w\nextrude 2\n"}],
        }),
    );
    assert!(client.diagnostics(&file).is_empty());
}

#[test]
fn lsp_document_symbols_list_the_bodies_of_examples() {
    let mut client = Client::start();
    let path = "examples/parts/enclosure.gcad";
    let file = uri(path);
    let text = std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    client.open(&file, &text);
    let symbols = client.request(
        "textDocument/documentSymbol",
        json!({"textDocument": {"uri": file}}),
    );
    let names: Vec<&str> = symbols
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"lid"), "{names:?}");
    assert!(names.contains(&"w"), "{names:?}");
    let lid = symbols
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "lid")
        .unwrap();
    let expected = text
        .lines()
        .position(|l| l.starts_with("body lid"))
        .unwrap();
    assert_eq!(lid["range"]["start"]["line"], expected);
}

#[test]
fn lsp_slow_build_does_not_stall_shutdown() {
    let mut client = Client::start();
    let path = "examples/mesh/hero.gcad";
    let file = uri(path);
    let text = std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    client.open(&file, &text);
    std::thread::sleep(Duration::from_millis(500));
    let start = Instant::now();
    client.request("shutdown", Value::Null);
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "{:?}",
        start.elapsed()
    );
    client.notify("exit", Value::Null);
}
