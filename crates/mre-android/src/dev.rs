//! Dev-client transport.
//!
//! The dev server writes a tiny pairing file into the app's private storage
//! (see `DevPairActivity`). This module reads it, fetches a length-prefixed Lua
//! bundle over plain HTTP with a token header, and long-polls for new
//! revisions. It uses only `std::net`, so the engine binary does not carry an
//! HTTP client or JSON parser for a feature that exists only in dev builds.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;
use winit::platform::android::activity::AndroidApp as WinitAndroidApp;

/// Pairing information written by the QR scanner Activity.
#[derive(Clone, Debug)]
pub struct DevConfig {
    pub host: String,
    pub port: u16,
    pub token: String,
    pub project: String,
}

/// One immutable bundle revision.
#[derive(Clone, Debug, Default)]
pub struct Bundle {
    pub revision: u64,
    pub modules: HashMap<String, String>,
}

impl Bundle {
    /// The Android host always executes `main.lua`.
    pub fn entry(&self) -> Option<&str> {
        self.modules.get("main.lua").map(String::as_str)
    }
}

/// Reads `dev-config.txt` from `Context.getFilesDir()`.
pub fn read_config(app: &WinitAndroidApp) -> Option<DevConfig> {
    let directory: PathBuf = app.internal_data_path()?;
    let text = std::fs::read_to_string(directory.join("dev-config.txt")).ok()?;
    parse_config(&text)
}

fn parse_config(text: &str) -> Option<DevConfig> {
    let mut host = None;
    let mut port = None;
    let mut token = None;
    let mut project = String::new();
    for line in text.lines() {
        let mut parts = line.splitn(2, ' ');
        let key = parts.next()?;
        let value = parts.next().unwrap_or("");
        match key {
            "protocol" if value != "mre-dev/1" => return None,
            "host" => host = Some(value.to_string()),
            "port" => port = value.parse().ok(),
            "token" => token = Some(value.to_string()),
            "project" => project = value.to_string(),
            _ => {}
        }
    }
    Some(DevConfig {
        host: host?,
        port: port?,
        token: token?,
        project,
    })
}

/// Result of a long poll.
#[derive(Debug)]
pub enum Wait {
    /// The server published a newer revision.
    Changed(u64),
    /// Nothing changed before the server's 25 s timeout.
    Unchanged,
}

/// Fetches the full bundle from the dev server.
pub fn fetch_bundle(config: &DevConfig) -> Result<Bundle, String> {
    let (status, body) = request(config, "GET", "/api/bundle", None, Duration::from_secs(10))?;
    if status != 200 {
        return Err(format!("bundle: HTTP {status}"));
    }
    parse_bundle(&body)
}

/// Waits until the server's revision is greater than `since`.
pub fn wait(config: &DevConfig, since: u64) -> Result<Wait, String> {
    let path = format!("/api/wait?since={since}");
    let (status, body) = request(config, "GET", &path, None, Duration::from_secs(28))?;
    match status {
        200 => {
            let text = String::from_utf8_lossy(&body);
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("revision ") {
                    return value
                        .parse()
                        .map(Wait::Changed)
                        .map_err(|_| format!("bad revision in {:?}", text));
                }
            }
            Err(format!("no revision in {:?}", text))
        }
        304 => Ok(Wait::Unchanged),
        other => Err(format!("wait: HTTP {other}")),
    }
}

/// Reports an applied revision back to the CLI. Fire-and-forget.
pub fn post_event(config: &DevConfig, event: &str) {
    let _ = request(
        config,
        "POST",
        "/api/events",
        Some(event),
        Duration::from_secs(2),
    );
}

fn request(
    config: &DevConfig,
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    let address: SocketAddr = (config.host.as_str(), config.port)
        .to_socket_addrs()
        .map_err(|error| format!("resolve {}: {}", config.host, error))?
        .next()
        .ok_or_else(|| format!("cannot resolve {}", config.host))?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .map_err(|error| format!("connect {address}: {error}"))?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();

    let length = body.map_or(0, str::len);
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nX-Mre-Token: {}\r\nConnection: close\r\n",
        config.host, config.port, config.token
    );
    if body.is_some() {
        request.push_str(&format!(
            "Content-Type: text/plain\r\nContent-Length: {length}\r\n"
        ));
    }
    request.push_str("\r\n");
    if let Some(body) = body {
        request.push_str(body);
    }
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("send request: {error}"))?;

    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader
        .read_line(&mut status)
        .map_err(|error| format!("read status: {error}"))?;
    let code = status
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("bad status line {status:?}"))?;

    let mut length = None;
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| format!("read headers: {error}"))?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().ok();
            }
        }
    }

    let mut body = Vec::new();
    if let Some(length) = length {
        if length > 0 {
            body.resize(length, 0);
            reader
                .read_exact(&mut body)
                .map_err(|error| format!("read body: {error}"))?;
        }
    } else {
        reader
            .read_to_end(&mut body)
            .map_err(|error| format!("read body: {error}"))?;
    }
    Ok((code, body))
}

fn parse_bundle(body: &[u8]) -> Result<Bundle, String> {
    let mut reader = BufReader::new(Cursor::new(body));
    let header = read_line(&mut reader)?;
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() != 3 || parts[0] != "MRE/1" {
        return Err(format!("bad bundle header {header:?}"));
    }
    let count: usize = parts[2]
        .parse()
        .map_err(|_| format!("bad module count in {header:?}"))?;
    let mut modules = HashMap::with_capacity(count);
    for _ in 0..count {
        let line = read_line(&mut reader)?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(format!("bad module header {line:?}"));
        }
        let name_length: usize = parts[0]
            .parse()
            .map_err(|_| format!("bad name length in {line:?}"))?;
        if parts[1].len() != name_length {
            return Err(format!("name length mismatch in {line:?}"));
        }
        let size: usize = parts[2]
            .parse()
            .map_err(|_| format!("bad body length in {line:?}"))?;
        let mut source = vec![0u8; size];
        reader
            .read_exact(&mut source)
            .map_err(|error| format!("module body: {error}"))?;
        // Go writes one separator newline after every module body.
        let mut separator = [0u8; 1];
        reader
            .read_exact(&mut separator)
            .map_err(|error| format!("module separator: {error}"))?;
        modules.insert(
            parts[1].to_string(),
            String::from_utf8(source).map_err(|error| format!("module is not UTF-8: {error}"))?,
        );
    }
    Ok(Bundle {
        revision: parts[1].parse().unwrap_or(0),
        modules,
    })
}

fn read_line(reader: &mut impl BufRead) -> Result<String, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|error| format!("read line: {error}"))?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}
