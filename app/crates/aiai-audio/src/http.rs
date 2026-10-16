//! Cliente HTTP/1.1 mínimo para o engine local (127.0.0.1): sem TLS, `Connection: close`, corpo por Content-Length,
//! chunked ou até fechar. Só aceita hosts locais — o áudio é sempre local (regra 2).

use crate::buf::{AudioError, AudioResult};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// `base` no formato `http://127.0.0.1:50021`. Devolve `host:porta`.
fn split_base(base: &str) -> AudioResult<String> {
    let rest = base.strip_prefix("http://").ok_or_else(|| AudioError::Http(format!("só http:// local: {base}")))?;
    let hostport = rest.trim_end_matches('/');
    let host = hostport.split(':').next().unwrap_or("");
    if !(host == "127.0.0.1" || host == "localhost" || host == "[::1]" || host.ends_with(".localhost")) {
        return Err(AudioError::Http(format!("host não local recusado: {host}")));
    }
    Ok(hostport.to_string())
}

pub fn request(base: &str, method: &str, path_and_query: &str, content_type: Option<&str>, body: &[u8], timeout: Duration) -> AudioResult<Response> {
    let hostport = split_base(base)?;
    let addr = hostport
        .to_socket_addrs()
        .map_err(|e| AudioError::Http(format!("resolver {hostport}: {e}")))?
        .next()
        .ok_or_else(|| AudioError::Http(format!("sem endereço para {hostport}")))?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| AudioError::Http(format!("conectar {hostport}: {e}")))?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(Duration::from_secs(30))).ok();
    let mut head = format!("{method} {path_and_query} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nAccept: */*\r\n");
    if let Some(ct) = content_type {
        head.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    s.write_all(head.as_bytes()).map_err(|e| AudioError::Http(e.to_string()))?;
    s.write_all(body).map_err(|e| AudioError::Http(e.to_string()))?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| AudioError::Http(format!("ler resposta: {e}")))?;
    parse_response(&raw)
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn parse_response(raw: &[u8]) -> AudioResult<Response> {
    let split = find(raw, b"\r\n\r\n").ok_or_else(|| AudioError::Http("resposta sem cabeçalho".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AudioError::Http(format!("status inválido: {status_line}")))?;
    let mut chunked = false;
    let mut content_length: Option<usize> = None;
    for l in lines {
        let lower = l.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("transfer-encoding:") {
            chunked = v.contains("chunked");
        } else if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().ok();
        }
    }
    let data = &raw[split + 4..];
    let body = if chunked {
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            let eol = find(&data[i..], b"\r\n").ok_or_else(|| AudioError::Http("chunk truncado".into()))? + i;
            let size_txt = String::from_utf8_lossy(&data[i..eol]).to_string();
            let size = usize::from_str_radix(size_txt.split(';').next().unwrap_or("").trim(), 16).map_err(|_| AudioError::Http("tamanho de chunk inválido".into()))?;
            i = eol + 2;
            if size == 0 {
                break;
            }
            if i + size > data.len() {
                return Err(AudioError::Http("chunk truncado".into()));
            }
            out.extend_from_slice(&data[i..i + size]);
            i += size + 2;
        }
        out
    } else if let Some(n) = content_length {
        if data.len() < n {
            return Err(AudioError::Http(format!("corpo truncado: {} de {n} bytes", data.len())));
        }
        data[..n].to_vec()
    } else {
        data.to_vec()
    };
    Ok(Response { status, body })
}

/// Escapa para query string (UTF-8 em %XX).
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_and_length() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"abcde"[..]));
        let r = parse_response(b"HTTP/1.1 422 X\r\nContent-Length: 2\r\n\r\nhi").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (422, &b"hi"[..]));
    }

    #[test]
    fn rejects_remote() {
        assert!(request("http://example.com", "GET", "/", None, &[], Duration::from_secs(1)).is_err());
        assert_eq!(url_encode("あ a"), "%E3%81%82%20a");
    }
}
