//! Conditional HTTP GET through nyquest's WinHTTP backend, which uses the system's TLS
//! stack and proxy settings.

use std::io;
use std::sync::Once;

use nyquest::ClientBuilder;
use nyquest::blocking::Request;

/// Where to fetch from. Production code can only build HTTPS endpoints.
#[derive(Debug, Clone)]
pub struct Endpoint {
    host: String,
    port: u16,
    secure: bool,
}

impl Endpoint {
    pub fn https(host: &str) -> Self {
        Endpoint {
            host: host.to_owned(),
            port: 443,
            secure: true,
        }
    }

    #[cfg(test)]
    fn plain_localhost(port: u16) -> Self {
        Endpoint {
            host: "127.0.0.1".to_owned(),
            port,
            secure: false,
        }
    }

    fn url(&self, path: &str) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        format!("{scheme}://{}:{}{path}", self.host, self.port)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fetch {
    /// The server answered 304: the copy matching the given ETag is current.
    NotModified,
    Downloaded {
        body: Vec<u8>,
        etag: Option<String>,
    },
}

/// GETs `path`. With `if_none_match`, the server may answer that nothing changed.
/// Bodies larger than `max_bytes` are refused.
pub fn get(
    endpoint: &Endpoint,
    path: &str,
    if_none_match: Option<&str>,
    max_bytes: usize,
) -> io::Result<Fetch> {
    // The ETag is read back from our own state file; still, never let it add headers.
    if if_none_match.is_some_and(|e| e.chars().any(|c| c.is_control())) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ETag contains control characters",
        ));
    }
    static REGISTER: Once = Once::new();
    REGISTER.call_once(nyquest_backend_winhttp::register);

    let client = ClientBuilder::default()
        .user_agent(concat!("procmon-mcp/", env!("CARGO_PKG_VERSION")))
        // The conditional request is ours to make; a cache must not answer it for us.
        .no_caching()
        .no_cookies()
        .max_response_buffer_size(max_bytes as u64)
        .build_blocking()
        .map_err(to_io)?;
    let mut request = Request::get(endpoint.url(path));
    if let Some(etag) = if_none_match {
        request = request.with_header("If-None-Match", etag.to_owned());
    }
    let response = client.request(request).map_err(to_io)?;
    match u16::from(response.status()) {
        304 => Ok(Fetch::NotModified),
        200 => {
            let etag = response
                .get_header("ETag")
                .map_err(to_io)?
                .into_iter()
                .next();
            let body = response.bytes().map_err(to_io)?;
            Ok(Fetch::Downloaded { body, etag })
        }
        other => Err(io::Error::other(format!("HTTP status {other}"))),
    }
}

fn to_io(e: nyquest::Error) -> io::Error {
    match e {
        nyquest::Error::Io(e) => e,
        other => io::Error::other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// Serves one request with `response` and reports the request's header lines.
    fn serve_once(response: &'static str) -> (Endpoint, mpsc::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                headers.push(line.trim_end().to_owned());
            }
            (&stream).write_all(response.as_bytes()).unwrap();
            tx.send(headers).unwrap();
        });
        (Endpoint::plain_localhost(port), rx)
    }

    fn has_header(headers: &[String], name: &str) -> Option<String> {
        headers.iter().find_map(|h| {
            let (n, v) = h.split_once(':')?;
            n.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
        })
    }

    #[test]
    fn downloads_body_and_etag_without_a_condition() {
        let (endpoint, seen) = serve_once(
            "HTTP/1.1 200 OK\r\nETag: \"abc\"\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
        );
        let fetched = get(&endpoint, "/Procmon64.exe", None, 1024).unwrap();
        assert_eq!(
            fetched,
            Fetch::Downloaded {
                body: b"hello".to_vec(),
                etag: Some("\"abc\"".to_owned())
            }
        );
        let headers = seen.recv().unwrap();
        assert_eq!(headers[0], "GET /Procmon64.exe HTTP/1.1");
        assert_eq!(has_header(&headers, "If-None-Match"), None);
    }

    #[test]
    fn sends_the_etag_and_accepts_not_modified() {
        let (endpoint, seen) =
            serve_once("HTTP/1.1 304 Not Modified\r\nETag: \"abc\"\r\nConnection: close\r\n\r\n");
        let fetched = get(&endpoint, "/Procmon64.exe", Some("\"abc\""), 1024).unwrap();
        assert_eq!(fetched, Fetch::NotModified);
        let headers = seen.recv().unwrap();
        assert_eq!(
            has_header(&headers, "If-None-Match").as_deref(),
            Some("\"abc\"")
        );
    }

    #[test]
    fn other_statuses_are_errors() {
        let (endpoint, _seen) =
            serve_once("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert!(get(&endpoint, "/missing", None, 1024).is_err());
    }

    #[test]
    fn refuses_bodies_over_the_limit() {
        let (endpoint, _seen) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        );
        assert!(get(&endpoint, "/big", None, 4).is_err());
    }

    #[test]
    fn rejects_an_etag_that_could_inject_headers() {
        let endpoint = Endpoint::https("example.invalid");
        assert!(get(&endpoint, "/", Some("\"a\"\r\nX-Evil: 1"), 1024).is_err());
    }
}
