//! HTTP/1.1 mínimo e ESTRITO (só o necessário para `POST /rpc` e `GET /health`), sem dependências.
//!
//! Não é um servidor HTTP genérico: sem keep-alive (sempre `Connection: close`), sem `Transfer-Encoding`,
//! sem `Expect`, sem CORS (qualquer `Origin` é recusado). Tudo que foge do esperado vira 4xx.
//! A lógica é pura (testável sem sockets); o servidor só alimenta as linhas.

pub const MAX_REQUEST_LINE: usize = 2048;
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
pub const MAX_HEADERS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    pub target: String,
    /// (nome em minúsculas, valor sem espaços nas pontas)
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status: u16,
    pub message: &'static str,
}

fn err(status: u16, message: &'static str) -> HttpError {
    HttpError { status, message }
}

/// Cabeçalhos que não podem aparecer duas vezes (evita ambiguidade/smuggling).
const SINGLETONS: &[&str] = &["host", "content-length", "content-type", "authorization", "origin", "transfer-encoding"];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

/// `lines[0]` = linha de requisição; o resto, cabeçalhos (sem CRLF, sem a linha em branco final).
pub fn parse_head(lines: &[String]) -> Result<Head, HttpError> {
    let first = lines.first().ok_or_else(|| err(400, "requisição vazia"))?;
    if first.len() > MAX_REQUEST_LINE {
        return Err(err(414, "linha de requisição longa demais"));
    }
    let mut parts = first.split(' ');
    let (method, target, version) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(m), Some(t), Some(v), None) => (m, t, v),
        _ => return Err(err(400, "linha de requisição inválida")),
    };
    if method.is_empty() || !method.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(err(400, "método inválido"));
    }
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(err(505, "versão HTTP não suportada"));
    }
    if !target.starts_with('/') || target.starts_with("//") || target.contains(|c: char| c.is_control() || c == '\\') {
        return Err(err(400, "alvo inválido"));
    }
    if lines.len() - 1 > MAX_HEADERS {
        return Err(err(431, "cabeçalhos demais"));
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in &lines[1..] {
        if line.starts_with(' ') || line.starts_with('\t') {
            return Err(err(400, "cabeçalho dobrado (obs-fold) não é aceito"));
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(err(400, "cabeçalho sem ':'"));
        };
        if name.is_empty() || !name.chars().all(is_token_char) {
            return Err(err(400, "nome de cabeçalho inválido"));
        }
        if value.contains(|c: char| (c.is_control() && c != '\t') || c == '\u{7f}') {
            return Err(err(400, "valor de cabeçalho inválido"));
        }
        let name = name.to_ascii_lowercase();
        if SINGLETONS.contains(&name.as_str()) && headers.iter().any(|(k, _)| *k == name) {
            return Err(err(400, "cabeçalho duplicado"));
        }
        headers.push((name, value.trim().to_string()));
    }
    Ok(Head { method: method.to_string(), target: target.to_string(), headers })
}

/// `Host` precisa ser EXATAMENTE um nome de loopback com a porta em que o servidor escuta
/// (proteção contra DNS rebinding: o pedido rebinded carrega o `Host` do atacante).
pub fn host_allowed(host: &str, port: u16) -> bool {
    let h = host.trim().to_ascii_lowercase();
    h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}") || h == format!("[::1]:{port}")
}

pub fn is_json_content_type(v: &str) -> bool {
    v.split(';').next().map(|m| m.trim().eq_ignore_ascii_case("application/json")).unwrap_or(false)
}

/// Extrai o token de `Authorization: Bearer <token>` (esquema case-insensitive).
pub fn bearer(v: &str) -> Option<&str> {
    let v = v.trim();
    let (scheme, rest) = v.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") && !rest.trim().is_empty() {
        Some(rest.trim())
    } else {
        None
    }
}

pub fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Error",
    }
}

/// Resposta HTTP completa (cabeçalhos + corpo). Sem CORS, sem cache, sem keep-alive.
pub fn response(status: u16, body: &[u8], extra: &[(&str, &str)]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
        status_text(status),
        body.len()
    )
    .into_bytes();
    for (k, v) in extra {
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

