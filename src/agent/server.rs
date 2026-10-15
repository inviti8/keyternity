//! The skinning agent's MCP server (docs/AI_SKINNING.md §7.2): Streamable HTTP on
//! 127.0.0.1, running while Keyternity is open, so Claude Code, Claude Desktop or any
//! MCP client can drive the tools in `agent`. Requests need the bearer token from
//! Settings. Tool calls are queued for the main thread (`poll`, once a frame) and
//! answered when they've run.
//!
//! Plain JSON responses only: no server-to-client stream (GET is refused), which the
//! transport allows for a server that never sends requests of its own. HTTP/1.1 on
//! the standard library's sockets: a thread per connection, keep-alive, and
//! Content-Length or chunked request bodies.

use super::{call, tools, Content, ToolResult, INSTRUCTIONS, WORKFLOW};
use crate::*;
use base64::Engine;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// The first port tried; the next nine are tried if it's taken.
pub const DEFAULT_PORT: u16 = 47870;

/// MCP protocol versions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

/// How long a tool call waits for the main thread.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Requests larger than this are refused.
const MAX_BODY: usize = 16 << 20;

struct Job {
    name: String,
    args: Value,
    reply: Sender<ToolResult>,
}

pub struct AgentServer {
    pub url: String,
    pub port: u16,
    pub token: String,
    jobs: Receiver<Job>,
    stop: Arc<AtomicBool>,
}

impl Drop for AgentServer {
    fn drop(&mut self) {
        // wake the accept loop so it sees the flag and lets go of the port
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// Listen on 127.0.0.1 from `port` (or the next free one of the following nine).
pub fn start(port: u16, token: String) -> Result<AgentServer, String> {
    let mut last = String::new();
    for p in port..port.saturating_add(10) {
        match TcpListener::bind(("127.0.0.1", p)) {
            Ok(listener) => {
                let (tx, rx) = channel();
                let stop = Arc::new(AtomicBool::new(false));
                let (s, t) = (stop.clone(), token.clone());
                std::thread::spawn(move || accept(listener, tx, t, s));
                return Ok(AgentServer { url: format!("http://127.0.0.1:{p}/mcp"), port: p, token, jobs: rx, stop });
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

/// A new random token for the server's bearer auth.
pub fn new_token() -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    h.update(now.as_nanos().to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    let local = 0u8;
    h.update((&local as *const u8 as usize).to_le_bytes());
    h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// Once a frame: start or stop the server as Settings say, and run queued tool calls.
pub fn poll(shared: &mut Shared) {
    shared.agent.read_only = shared.config.agent_read_only;
    if !shared.config.agent_access {
        shared.agent.server = None;
        shared.agent.start_error = None;
        shared.ui.agent_url = None;
        shared.ui.agent_error = None;
        return;
    }
    if shared.config.agent_token.is_empty() {
        shared.config.agent_token = new_token();
        utils::save_config(&shared.config);
    }
    // settings changed: restart
    let port = if shared.config.agent_port == 0 { DEFAULT_PORT } else { shared.config.agent_port };
    if let Some(s) = &shared.agent.server {
        if s.token != shared.config.agent_token || s.port < port || s.port >= port.saturating_add(10) {
            shared.agent.server = None;
            shared.agent.start_error = None;
        }
    }
    if shared.agent.server.is_none() && shared.agent.start_error.is_none() {
        match start(port, shared.config.agent_token.clone()) {
            Ok(s) => shared.agent.server = Some(s),
            Err(e) => shared.agent.start_error = Some(e),
        }
    }

    shared.ui.agent_url = shared.agent.server.as_ref().map(|s| s.url.clone());
    shared.ui.agent_error = shared.agent.start_error.clone();

    let Some(server) = shared.agent.server.take() else { return };
    while let Ok(job) = server.jobs.try_recv() {
        let result = call(shared, &job.name, &job.args);
        let _ = job.reply.send(result);
    }
    shared.agent.server = Some(server);
}

fn accept(listener: TcpListener, jobs: Sender<Job>, token: String, stop: Arc<AtomicBool>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else { continue };
        let (jobs, token, stop) = (jobs.clone(), token.clone(), stop.clone());
        std::thread::spawn(move || connection(stream, jobs, token, stop));
    }
}

/// One client connection: requests one after another (keep-alive) until it closes,
/// goes quiet for a minute, or the server stops.
fn connection(stream: TcpStream, jobs: Sender<Job>, token: String, stop: Arc<AtomicBool>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let mut out = stream;
    while let Ok(Some(request)) = read_request(&mut reader) {
        let close = request.header("Connection").map_or(false, |c| c.eq_ignore_ascii_case("close"));
        let (status, body) = handle(&request, &jobs, &token);
        if write_response(&mut out, status, body, close).is_err() || close || stop.load(Ordering::SeqCst) {
            break;
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// The next HTTP/1.1 request on a connection; `None` when the client closed it.
fn read_request(reader: &mut impl BufRead) -> std::io::Result<Option<Request>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = vec![];
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            return Ok(None);
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut request = Request { method, path, headers, body: vec![] };
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "bad request");
    let chunked = request.header("Transfer-Encoding").map_or(false, |t| t.eq_ignore_ascii_case("chunked"));
    if chunked {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size)?;
            let n = usize::from_str_radix(size.trim().split(';').next().unwrap_or(""), 16).map_err(|_| bad())?;
            if n == 0 {
                let mut end = String::new();
                reader.read_line(&mut end)?;
                break;
            }
            if request.body.len() + n > MAX_BODY {
                return Err(bad());
            }
            let mut chunk = vec![0; n + 2];
            reader.read_exact(&mut chunk)?;
            request.body.extend_from_slice(&chunk[..n]);
        }
    } else if let Some(len) = request.header("Content-Length") {
        let n: usize = len.parse().map_err(|_| bad())?;
        if n > MAX_BODY {
            return Err(bad());
        }
        request.body = vec![0; n];
        reader.read_exact(&mut request.body)?;
    }
    Ok(Some(request))
}

fn write_response(out: &mut TcpStream, status: u16, body: Option<Value>, close: bool) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let data = body.as_ref().map_or(String::new(), |b| b.to_string());
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n", data.len());
    if body.is_some() {
        head += "Content-Type: application/json\r\n";
    }
    if status == 405 {
        head += "Allow: POST\r\n";
    }
    head += if close { "Connection: close\r\n\r\n" } else { "Connection: keep-alive\r\n\r\n" };
    out.write_all(head.as_bytes())?;
    out.write_all(data.as_bytes())?;
    out.flush()
}

fn handle(request: &Request, jobs: &Sender<Job>, token: &str) -> (u16, Option<Value>) {
    // only clients that know the token, and no web pages (DNS rebinding)
    if request.header("Authorization") != Some(&format!("Bearer {token}")) {
        return (401, Some(json!({ "error": "missing or wrong token; see Keyternity's Settings > Agent access" })));
    }
    if let Some(origin) = request.header("Origin") {
        let local = ["http://127.0.0.1", "http://localhost", "https://127.0.0.1", "https://localhost"];
        if !local.iter().any(|l| origin.starts_with(l)) {
            return (403, None);
        }
    }
    if !request.path.starts_with("/mcp") {
        return (404, None);
    }
    if request.method != "POST" {
        return (405, None);
    }

    let Ok(msg) = serde_json::from_slice::<Value>(&request.body) else {
        return (400, Some(rpc_error(Value::Null, -32700, "parse error")));
    };
    // notifications and responses get no answer
    let Some(id) = msg.get("id").cloned() else {
        return (202, None);
    };
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("");
            let version = if PROTOCOL_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSIONS[0] };
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {}, "prompts": {} },
                "serverInfo": { "name": "keyternity", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": tools().iter().map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.schema })).collect::<Vec<_>>()
        })),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (tx, rx) = channel();
            let _ = jobs.send(Job { name, args, reply: tx });
            let result = rx.recv_timeout(CALL_TIMEOUT).unwrap_or_else(|_| ToolResult {
                content: vec![Content::Text("Keyternity didn't run the tool in time. Is its window open (not minimized)?".into())],
                is_error: true,
            });
            Ok(result_json(&result))
        }
        "prompts/list" => Ok(json!({ "prompts": [{
            "name": "skin_character",
            "description": "Skin the rig open in Keyternity: bind, assign drawn parts to bones, test poses, fix, report.",
            "arguments": [{ "name": "notes", "description": "anything the agent should know (e.g. keep the bag rigid)", "required": false }]
        }]})),
        "prompts/get" => {
            let notes = params.pointer("/arguments/notes").and_then(|n| n.as_str()).unwrap_or("");
            let text = if notes.is_empty() { WORKFLOW.to_string() } else { format!("{WORKFLOW}\n\nArtist's notes: {notes}") };
            Ok(json!({
                "description": "Skin the open rig",
                "messages": [{ "role": "user", "content": { "type": "text", "text": text } }]
            }))
        }
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    match result {
        Ok(r) => (200, Some(json!({ "jsonrpc": "2.0", "id": id, "result": r }))),
        Err((code, m)) => (200, Some(rpc_error(id, code, &m))),
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result_json(r: &ToolResult) -> Value {
    let content: Vec<Value> = r
        .content
        .iter()
        .map(|c| match c {
            Content::Text(t) => json!({ "type": "text", "text": t }),
            Content::Image(png) => json!({
                "type": "image",
                "data": base64::engine::general_purpose::STANDARD.encode(png),
                "mimeType": "image/png"
            }),
        })
        .collect();
    json!({ "content": content, "isError": r.is_error })
}
