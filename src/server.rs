//! Yalnızca 127.0.0.1'de dinleyen küçük HTTP/1.1 sunucusu: eklentinin /poll, /report, /status
//! ve /trace istekleri. Her bağlantı kendi iş parçacığında, tek istekten sonra kapanır.
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::bridge::Api;

pub const PORT: u16 = 47831;
const MAX_CONNECTIONS: usize = 32;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY: usize = 262_144;

pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> &str {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or("", |(_, v)| v.as_str())
    }
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    }
}

/// `localhost:47831` gibi bağlanılmış dinleyiciyi açar; port doluysa hata döner.
pub fn bind(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", port))
}

pub fn serve<A: Api>(listener: TcpListener, api: Arc<A>, token: Arc<str>) {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            continue; // bağlantı düşer; eklenti yeniden dener
        }
        let (api, token, done) = (Arc::clone(&api), Arc::clone(&token), Arc::clone(&active));
        let spawned = thread::Builder::new().stack_size(128 * 1024).spawn(move || {
            let _ = handle_connection(stream, &*api, &token);
            done.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn handle_connection(stream: TcpStream, api: &dyn Api, token: &str) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut writer = stream.try_clone()?;
    let response = match read_request(BufReader::new(stream)) {
        Ok(request) => route(&request, api, token),
        Err(ReadError::Io) => return Ok(()),
        Err(ReadError::Bad(status, message)) => reply(status, "", json!({"error": message})),
    };
    write_response(&mut writer, &response)?;
    writer.flush()?;
    let _ = writer.shutdown(Shutdown::Both);
    Ok(())
}

pub enum ReadError {
    Io,
    Bad(u16, &'static str),
}

pub fn read_request(mut reader: impl BufRead) -> Result<Request, ReadError> {
    let mut head = Vec::new();
    // Başlıklar boş satıra kadar; sınırı aşan istek reddedilir.
    loop {
        let before = head.len();
        let n = (&mut reader).take((MAX_HEADER_BYTES + 1 - before) as u64)
            .read_until(b'\n', &mut head).map_err(|_| ReadError::Io)?;
        if n == 0 {
            return Err(ReadError::Io);
        }
        if head.len() > MAX_HEADER_BYTES {
            return Err(ReadError::Bad(413, "İstek boyutu geçersiz."));
        }
        let line = &head[before..];
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let head = String::from_utf8(head).map_err(|_| ReadError::Bad(400, "İstek geçersiz."))?;
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target) = match (first.next(), first.next()) {
        (Some(m), Some(t)) if !m.is_empty() && t.starts_with('/') => (m.to_string(), t.to_string()),
        _ => return Err(ReadError::Bad(400, "İstek geçersiz.")),
    };
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let mut request = Request { method, target, headers, body: Vec::new() };
    let length = request.header("Content-Length");
    if !length.is_empty() {
        let size: usize = length.parse().map_err(|_| ReadError::Bad(400, "İstek boyutu geçersiz."))?;
        if size > MAX_BODY {
            return Err(ReadError::Bad(400, "İstek boyutu geçersiz."));
        }
        request.body = vec![0; size];
        reader.read_exact(&mut request.body).map_err(|_| ReadError::Bad(408, "İstek zaman aşımına uğradı."))?;
    }
    Ok(request)
}

pub fn write_response(out: &mut impl Write, r: &Response) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\nConnection: close\r\n", r.status, reason(r.status));
    for (k, v) in &r.headers {
        head += &format!("{k}: {v}\r\n");
    }
    if r.status != 204 {
        head += &format!("Content-Length: {}\r\n", r.body.len());
    }
    head += "\r\n";
    out.write_all(head.as_bytes())?;
    out.write_all(&r.body)
}

fn extension_origin(origin: &str) -> bool {
    origin.starts_with("chrome-extension://")
}

fn reply(status: u16, origin: &str, payload: Value) -> Response {
    let mut headers = vec![
        ("Content-Type", "application/json; charset=utf-8".to_string()),
        ("Cache-Control", "no-store".to_string()),
    ];
    if extension_origin(origin) {
        headers.push(("Access-Control-Allow-Origin", origin.to_string()));
        headers.push(("Vary", "Origin".to_string()));
    }
    Response { status, headers, body: payload.to_string().into_bytes() }
}

/// Zamanlama farkı sızdırmayan karşılaştırma.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn authorized(request: &Request, token: &str) -> bool {
    let origin = request.header("Origin");
    (origin.is_empty() || extension_origin(origin))
        && same(request.header("Authorization").as_bytes(), format!("Bearer {token}").as_bytes())
}

/// `a=1&b=x%20y` biçimini çözer (`+` boşluktur); geçersiz UTF-8 yer tutucuyla değişir.
pub fn query_param(query: &str, name: &str) -> String {
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if decode(k) == name {
            return decode(v);
        }
    }
    String::new()
}

fn decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        (b as char).to_digit(16).map(|d| d as u8)
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => match (bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
                (Some(hi), Some(lo)) => {
                    out.push(hi << 4 | lo);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn route(request: &Request, api: &dyn Api, token: &str) -> Response {
    let origin = request.header("Origin");
    if request.method == "OPTIONS" {
        if !extension_origin(origin) {
            return reply(403, origin, json!({"error": "Erişim reddedildi."}));
        }
        return Response {
            status: 204,
            headers: vec![
                ("Access-Control-Allow-Origin", origin.to_string()),
                ("Access-Control-Allow-Methods", "GET, POST, OPTIONS".into()),
                ("Access-Control-Allow-Headers", "Authorization, Content-Type".into()),
            ],
            body: Vec::new(),
        };
    }
    if request.method != "GET" && request.method != "POST" {
        return reply(404, origin, json!({"error": "Bulunamadı."}));
    }
    if !authorized(request, token) {
        return reply(403, origin, json!({"error": "Erişim reddedildi."}));
    }
    let (path, query) = request.target.split_once('?').unwrap_or((&request.target, ""));
    if request.method == "GET" {
        return match path {
            "/status" => reply(200, origin, api.status()),
            "/poll" => match api.poll(&query_param(query, "client"), &query_param(query, "v"),
                                      &query_param(query, "mic")) {
                Ok(payload) => reply(200, origin, payload),
                Err(error) => reply(400, origin, json!({"error": error})),
            },
            "/trace" => {
                let client = query_param(query, "client");
                api.trace(if client.is_empty() { "?" } else { &client }, &query_param(query, "msg"));
                reply(200, origin, json!({"ok": true}))
            }
            _ => reply(404, origin, json!({"error": "Bulunamadı."})),
        };
    }
    if request.target != "/report" {
        return reply(404, origin, json!({"error": "Bulunamadı."}));
    }
    if request.body.is_empty() {
        return reply(400, origin, json!({"error": "İstek boyutu geçersiz."}));
    }
    match serde_json::from_slice::<Value>(&request.body) {
        Ok(data) if data.is_object() => {
            let client = data.get("client").and_then(Value::as_str);
            reply(200, origin, api.report(client, &data))
        }
        _ => reply(400, origin, json!({"error": "İstek geçersiz."})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeApi {
        polls: Mutex<Vec<(String, String, String)>>,
        reports: Mutex<Vec<Value>>,
        traces: Mutex<Vec<String>>,
    }

    impl Api for FakeApi {
        fn status(&self) -> Value {
            json!({"connected": false, "phase": "idle"})
        }
        fn poll(&self, client: &str, version: &str, mic: &str) -> Result<Value, String> {
            self.polls.lock().unwrap().push((client.into(), version.into(), mic.into()));
            Ok(json!({"command": null}))
        }
        fn report(&self, _: Option<&str>, data: &Value) -> Value {
            self.reports.lock().unwrap().push(data.clone());
            json!({"ok": true})
        }
        fn trace(&self, client: &str, msg: &str) {
            self.traces.lock().unwrap().push(format!("{client}: {msg}"));
        }
    }

    /// Gerçek bir TCP bağlantısı üzerinden ham istek gönderir; (durum, başlıklar, gövde) döner.
    fn send(port: u16, raw: &str) -> (u16, String, Value) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        let status = head[9..12].parse().unwrap();
        (status, head.to_string(), serde_json::from_str(body).unwrap_or(Value::Null))
    }

    fn start() -> (u16, Arc<FakeApi>) {
        let listener = bind(0).unwrap();
        let port = listener.local_addr().unwrap().port();
        let api = Arc::new(FakeApi::default());
        let served = Arc::clone(&api);
        thread::spawn(move || serve(listener, served, Arc::from("test-token")));
        (port, api)
    }

    const AUTH: &str = "Authorization: Bearer test-token\r\nOrigin: chrome-extension://test\r\n";

    #[test]
    fn rejects_unauthorized_web_origins_and_routes() {
        let (port, api) = start();
        for headers in ["", "Authorization: Bearer wrong\r\n", "Authorization: Bearer test-token\r\nOrigin: https://chatgpt.com\r\n",
                        "Authorization: Bearer test-token2\r\n", "authorization: bearer test-token\r\n"] {
            let (status, _, _) = send(port, &format!("GET /status HTTP/1.1\r\n{headers}\r\n"));
            assert_eq!(status, 403, "{headers}");
        }
        let (status, head, body) = send(port, &format!("GET /status HTTP/1.1\r\n{AUTH}\r\n"));
        assert_eq!((status, body), (200, json!({"connected": false, "phase": "idle"})));
        assert!(head.contains("Access-Control-Allow-Origin: chrome-extension://test"));
        assert!(head.contains("Cache-Control: no-store"));

        // Origin olmadan (ör. curl) anahtar yeterli.
        let (status, head, _) = send(port, "GET /status HTTP/1.1\r\nAuthorization: Bearer test-token\r\n\r\n");
        assert_eq!(status, 200);
        assert!(!head.contains("Access-Control-Allow-Origin"));

        let (status, _, body) = send(port, &format!("GET /poll?client=tab-aaaaaaaa&v=2.2.0&mic=granted HTTP/1.1\r\n{AUTH}\r\n"));
        assert_eq!((status, body), (200, json!({"command": null})));
        assert_eq!(*api.polls.lock().unwrap(), vec![("tab-aaaaaaaa".into(), "2.2.0".into(), "granted".into())]);

        let report = r#"{"client":"tab-aaaaaaaa","session":1,"event":"result","text":"Türkçe x"}"#;
        let (status, _, body) = send(port, &format!(
            "POST /report HTTP/1.1\r\n{AUTH}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{report}", report.len()));
        assert_eq!((status, body), (200, json!({"ok": true})));
        assert_eq!(api.reports.lock().unwrap()[0]["text"], "Türkçe x");

        let (status, _, _) = send(port, &format!("GET /nope HTTP/1.1\r\n{AUTH}\r\n"));
        assert_eq!(status, 404);
        let (status, _, _) = send(port, &format!("POST /report?x=1 HTTP/1.1\r\n{AUTH}Content-Length: 2\r\n\r\n{{}}"));
        assert_eq!(status, 404);
        let (status, _, _) = send(port, &format!("POST /report HTTP/1.1\r\n{AUTH}Content-Length: 5\r\n\r\n[1,2]"));
        assert_eq!(status, 400);
        let (status, _, _) = send(port, &format!("POST /report HTTP/1.1\r\n{AUTH}\r\n"));
        assert_eq!(status, 400);
        let (status, _, _) = send(port, &format!("POST /report HTTP/1.1\r\n{AUTH}Content-Length: 999999\r\n\r\n"));
        assert_eq!(status, 400);
    }

    #[test]
    fn cors_preflight_only_for_extensions() {
        let (port, _) = start();
        let (status, head, _) = send(port, "OPTIONS /report HTTP/1.1\r\nOrigin: chrome-extension://abc\r\n\r\n");
        assert_eq!(status, 204);
        assert!(head.contains("Access-Control-Allow-Origin: chrome-extension://abc"));
        assert!(head.contains("Access-Control-Allow-Methods: GET, POST, OPTIONS"));
        assert!(head.contains("Access-Control-Allow-Headers: Authorization, Content-Type"));
        let (status, _, _) = send(port, "OPTIONS /report HTTP/1.1\r\nOrigin: https://evil.example\r\n\r\n");
        assert_eq!(status, 403);
    }

    #[test]
    fn trace_is_decoded() {
        let (port, api) = start();
        let (status, _, _) = send(port, &format!("GET /trace?client=tab-aaaaaaaa&msg=sonu%C3%A7+5%20karakter HTTP/1.1\r\n{AUTH}\r\n"));
        assert_eq!(status, 200);
        assert_eq!(*api.traces.lock().unwrap(), vec!["tab-aaaaaaaa: sonuç 5 karakter".to_string()]);
    }

    #[test]
    fn query_decoding() {
        assert_eq!(query_param("client=a%2Fb&v=", "client"), "a/b");
        assert_eq!(query_param("client=a&v=", "v"), "");
        assert_eq!(query_param("x=%zz%4", "x"), "%zz%4");
        assert_eq!(query_param("", "client"), "");
    }
}
