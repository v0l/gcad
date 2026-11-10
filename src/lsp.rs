use anyhow::Result;
use lsp_server::{Connection, Message, Notification, Response};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(300);
const ABORTED: &str = "a newer build took over";
const ERROR: u8 = 1;
const WARNING: u8 = 2;

struct Diagnostic {
    line: usize,
    severity: u8,
    message: String,
}

struct Job {
    uri: String,
    text: String,
}

fn line_of(error: &str) -> usize {
    error
        .split("line ")
        .skip(1)
        .find_map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<usize>().ok()
        })
        .map(|n| n.saturating_sub(1))
        .unwrap_or(0)
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn failure(error: &anyhow::Error) -> Vec<Diagnostic> {
    let message = format!("{error:#}");
    vec![Diagnostic {
        line: line_of(&message),
        severity: ERROR,
        message,
    }]
}

fn diagnose_drawing(path: &Path, text: &str) -> Vec<Diagnostic> {
    match gcad::drafting::run_source(path, text, &[]) {
        Err(error) => failure(&error),
        Ok(drawing) => drawing
            .failed()
            .map(|(line, error)| {
                vec![Diagnostic {
                    line: line.number.saturating_sub(1),
                    severity: ERROR,
                    message: format!("{error:#}"),
                }]
            })
            .unwrap_or_default(),
    }
}

fn diagnose_model(path: &Path, text: &str) -> Vec<Diagnostic> {
    let run = match gcad::model::run_source(path, text, &[]) {
        Ok(run) => run,
        Err(error) => return failure(&error),
    };
    let mut found = Vec::new();
    if let Some((line, Err(error))) = run.steps.last() {
        found.push(Diagnostic {
            line: line.number.saturating_sub(1),
            severity: ERROR,
            message: format!("{error:#}"),
        });
        return found;
    }
    let last = text.lines().count().saturating_sub(1);
    let bodies: Vec<String> = run
        .model
        .body_names()
        .into_iter()
        .filter(|name| !run.model.members.contains(name))
        .collect();
    if !run.model.assembly && bodies.len() > 1 {
        found.push(Diagnostic {
            line: 0,
            severity: WARNING,
            message: format!(
                "{} separate bodies ({}). If they are separate parts, bring this file into a .gasm with `part` to place, join and check them; if they are one part, `combine` them",
                bodies.len(),
                bodies.join(", ")
            ),
        });
    }
    if let Some(spec) = gcad::sim::spec(&run.model) {
        if let Err(error) = spec.target() {
            found.push(Diagnostic {
                line: last,
                severity: ERROR,
                message: format!("{error:#}"),
            });
        }
    } else if run.model.solids().is_empty() && run.model.meshes().is_empty() {
        found.push(Diagnostic {
            line: last,
            severity: ERROR,
            message: "the file builds no solid".to_string(),
        });
    }
    found
}

#[cfg(feature = "game-assets")]
fn diagnose_asset(path: &Path) -> Vec<Diagnostic> {
    match gcad::asset::author::load(path, &[]) {
        Ok(_) => Vec::new(),
        Err(error) => failure(&error),
    }
}

#[cfg(not(feature = "game-assets"))]
fn diagnose_asset(_path: &Path) -> Vec<Diagnostic> {
    Vec::new()
}

fn diagnose(path: &Path, text: &str) -> Vec<Diagnostic> {
    match extension(path).as_str() {
        "gdwg" => diagnose_drawing(path, text),
        "gasset" => diagnose_asset(path),
        _ => diagnose_model(path, text),
    }
}

fn aborted(found: &[Diagnostic]) -> bool {
    found.iter().any(|d| d.message == ABORTED)
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut k = 0;
    while k < bytes.len() {
        if bytes[k] == b'%'
            && k + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&text[k + 1..k + 3], 16)
        {
            out.push(byte);
            k += 3;
        } else {
            out.push(bytes[k]);
            k += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn path_of(uri: &str) -> PathBuf {
    PathBuf::from(decode(uri.strip_prefix("file://").unwrap_or(uri)))
}

fn publish(connection: &Sender, uri: &str, found: &[Diagnostic], text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    let diagnostics: Vec<Value> = found
        .iter()
        .map(|d| {
            let width = lines
                .get(d.line)
                .map(|l| l.encode_utf16().count())
                .unwrap_or(0);
            json!({
                "range": {
                    "start": {"line": d.line, "character": 0},
                    "end": {"line": d.line, "character": width},
                },
                "severity": d.severity,
                "source": "gcad",
                "message": d.message,
            })
        })
        .collect();
    let notification = Notification::new(
        "textDocument/publishDiagnostics".to_string(),
        json!({"uri": uri, "diagnostics": diagnostics}),
    );
    let _ = connection.send(Message::Notification(notification));
}

type Sender = crossbeam_channel::Sender<Message>;

fn worker(jobs: mpsc::Receiver<Job>, out: Sender) {
    let mut queue: Vec<Job> = Vec::new();
    loop {
        if queue.is_empty() {
            match jobs.recv() {
                Ok(job) => queue.push(job),
                Err(_) => return,
            }
        }
        while let Ok(job) = jobs.try_recv() {
            queue.retain(|queued| queued.uri != job.uri);
            queue.push(job);
        }
        let job = queue.remove(0);
        let path = path_of(&job.uri);
        let found = gcad::host::superseding(|| diagnose(&path, &job.text));
        if aborted(&found) {
            queue.insert(0, job);
            continue;
        }
        publish(&out, &job.uri, &found, &job.text);
    }
}

fn symbols(text: &str) -> Vec<Value> {
    let mut found = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Ok(line) = gcad::parse::parse_line(index + 1, raw) else {
            continue;
        };
        let names: Vec<(String, u8)> = match line.op.as_str() {
            "body" => line
                .positional
                .first()
                .map(|n| (n.clone(), 23))
                .into_iter()
                .collect(),
            "part" => line
                .positional
                .first()
                .map(|n| (n.clone(), 2))
                .into_iter()
                .collect(),
            "let" => line.named.iter().map(|(k, _)| (k.clone(), 13)).collect(),
            _ => Vec::new(),
        };
        let width = raw.encode_utf16().count();
        for (name, kind) in names {
            let range = json!({
                "start": {"line": index, "character": 0},
                "end": {"line": index, "character": width},
            });
            found.push(json!({
                "name": name,
                "detail": line.op,
                "kind": kind,
                "range": range,
                "selectionRange": range,
            }));
        }
    }
    found
}

fn word_at(text: &str, line: usize, character: usize) -> Option<String> {
    let chars: Vec<char> = text.lines().nth(line)?.chars().collect();
    let word = |c: &char| c.is_ascii_alphanumeric() || *c == '_';
    let mut start = character.min(chars.len());
    let mut end = start;
    while start > 0 && word(&chars[start - 1]) {
        start -= 1;
    }
    while end < chars.len() && word(&chars[end]) {
        end += 1;
    }
    (start < end).then(|| chars[start..end].iter().collect())
}

fn hover(text: &str, line: usize, character: usize) -> Value {
    let Some(word) = word_at(text, line, character) else {
        return Value::Null;
    };
    let marker = format!("| `{word}` |");
    let rows: Vec<&str> = include_str!("../docs/format.md")
        .lines()
        .filter(|row| row.starts_with(&marker))
        .collect();
    if rows.is_empty() {
        return Value::Null;
    }
    json!({"contents": {"kind": "markdown", "value": rows.join("\n")}})
}

fn position(params: &Value) -> (usize, usize) {
    let at = |key: &str| params["position"][key].as_u64().unwrap_or(0) as usize;
    (at("line"), at("character"))
}

pub fn serve() -> Result<()> {
    let (connection, _threads) = Connection::stdio();
    let (id, _params) = connection.initialize_start()?;
    connection.initialize_finish(
        id,
        json!({
            "capabilities": {
                "textDocumentSync": {"openClose": true, "change": 1, "save": {}},
                "documentSymbolProvider": true,
                "hoverProvider": true,
            },
            "serverInfo": {"name": "gcad", "version": env!("CARGO_PKG_VERSION")},
        }),
    )?;
    let (jobs, queue) = mpsc::channel();
    let out = connection.sender.clone();
    std::thread::spawn(move || worker(queue, out));
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut due: HashMap<String, Instant> = HashMap::new();
    loop {
        let timeout = due
            .values()
            .min()
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));
        match connection.receiver.recv_timeout(timeout) {
            Ok(Message::Request(request)) => {
                let result = match request.method.as_str() {
                    "shutdown" => Value::Null,
                    "textDocument/documentSymbol" => {
                        let uri = request.params["textDocument"]["uri"].as_str().unwrap_or("");
                        json!(symbols(
                            documents.get(uri).map(String::as_str).unwrap_or("")
                        ))
                    }
                    "textDocument/hover" => {
                        let uri = request.params["textDocument"]["uri"].as_str().unwrap_or("");
                        let (line, character) = position(&request.params);
                        hover(
                            documents.get(uri).map(String::as_str).unwrap_or(""),
                            line,
                            character,
                        )
                    }
                    _ => {
                        let _ = connection.sender.send(Message::Response(Response::new_err(
                            request.id,
                            -32601,
                            format!("unsupported request {}", request.method),
                        )));
                        continue;
                    }
                };
                let _ = connection
                    .sender
                    .send(Message::Response(Response::new_ok(request.id, result)));
            }
            Ok(Message::Notification(note)) => {
                let uri = note.params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                match note.method.as_str() {
                    "exit" => break,
                    "textDocument/didOpen" => {
                        let text = note.params["textDocument"]["text"].as_str().unwrap_or("");
                        documents.insert(uri.clone(), text.to_string());
                        due.insert(uri, Instant::now());
                    }
                    "textDocument/didChange" => {
                        if let Some(text) = note.params["contentChanges"]
                            .as_array()
                            .and_then(|changes| changes.last())
                            .and_then(|change| change["text"].as_str())
                        {
                            documents.insert(uri.clone(), text.to_string());
                            due.insert(uri, Instant::now() + DEBOUNCE);
                        }
                    }
                    "textDocument/didSave" => {
                        if let Some(text) = note.params["text"].as_str() {
                            documents.insert(uri.clone(), text.to_string());
                        }
                        if documents.contains_key(&uri) {
                            due.insert(uri, Instant::now());
                        }
                    }
                    "textDocument/didClose" => {
                        documents.remove(&uri);
                        due.remove(&uri);
                        publish(&connection.sender, &uri, &[], "");
                    }
                    _ => {}
                }
            }
            Ok(Message::Response(_)) => {}
            Err(mpsc_timeout) if mpsc_timeout.is_timeout() => {}
            Err(_) => break,
        }
        let now = Instant::now();
        let ready: Vec<String> = due
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(uri, _)| uri.clone())
            .collect();
        for uri in ready {
            due.remove(&uri);
            if let Some(text) = documents.get(&uri) {
                gcad::host::superseding(|| ());
                let _ = jobs.send(Job {
                    uri,
                    text: text.clone(),
                });
            }
        }
    }
    std::process::exit(0);
}
