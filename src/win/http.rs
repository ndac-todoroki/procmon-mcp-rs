//! Conditional HTTP GET over WinHTTP, which uses the system's TLS and proxy settings.

use std::ffi::c_void;
use std::io;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Networking::WinHttp::{
    ERROR_WINHTTP_HEADER_NOT_FOUND, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_ETAG, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
};

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
    let agent = wide(concat!("procmon-mcp/", env!("CARGO_PKG_VERSION")));
    // SAFETY: every handle is wrapped in `Handle`, which closes it on all paths.
    unsafe {
        let session = Handle::new(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        ))?;
        let host = wide(&endpoint.host);
        let connection = Handle::new(WinHttpConnect(session.0, host.as_ptr(), endpoint.port, 0))?;
        let verb = wide("GET");
        let object = wide(path);
        let flags = if endpoint.secure {
            WINHTTP_FLAG_SECURE
        } else {
            0
        };
        let request = Handle::new(WinHttpOpenRequest(
            connection.0,
            verb.as_ptr(),
            object.as_ptr(),
            null(),
            null(),
            null(),
            flags,
        ))?;
        if let Some(etag) = if_none_match {
            let header: Vec<u16> = format!("If-None-Match: {etag}").encode_utf16().collect();
            check(WinHttpAddRequestHeaders(
                request.0,
                header.as_ptr(),
                header.len() as u32,
                WINHTTP_ADDREQ_FLAG_ADD,
            ))?;
        }
        check(WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0))?;
        check(WinHttpReceiveResponse(request.0, null_mut()))?;

        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        check(WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&raw mut status).cast(),
            &mut size,
            null_mut(),
        ))?;
        match status {
            304 => Ok(Fetch::NotModified),
            200 => {
                let etag = query_string_header(request.0, WINHTTP_QUERY_ETAG)?;
                let body = read_body(request.0, max_bytes)?;
                Ok(Fetch::Downloaded { body, etag })
            }
            other => Err(io::Error::other(format!("HTTP status {other}"))),
        }
    }
}

/// # Safety
/// `request` must be an open WinHTTP request handle with a received response.
unsafe fn read_body(request: *mut c_void, max_bytes: usize) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let mut read = 0u32;
        // SAFETY: guaranteed by the caller; the buffer length is passed with it.
        check(unsafe {
            WinHttpReadData(
                request,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )
        })?;
        if read == 0 {
            return Ok(body);
        }
        if body.len() + read as usize > max_bytes {
            return Err(io::Error::other(format!(
                "response larger than {max_bytes} bytes"
            )));
        }
        body.extend_from_slice(&chunk[..read as usize]);
    }
}

/// A response header as text, or `None` when the server did not send it.
///
/// # Safety
/// `request` must be an open WinHTTP request handle with a received response.
unsafe fn query_string_header(request: *mut c_void, info: u32) -> io::Result<Option<String>> {
    let mut buf = [0u16; 512];
    let mut size = size_of_val(&buf) as u32;
    // SAFETY: guaranteed by the caller; `size` is the buffer size in bytes.
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            info,
            null(),
            buf.as_mut_ptr().cast(),
            &mut size,
            null_mut(),
        )
    };
    if ok == 0 {
        let err = io::Error::last_os_error();
        return if err.raw_os_error() == Some(ERROR_WINHTTP_HEADER_NOT_FOUND as i32) {
            Ok(None)
        } else {
            Err(err)
        };
    }
    // `size` is now the length in bytes, excluding the terminating NUL.
    Ok(Some(String::from_utf16_lossy(&buf[..size as usize / 2])))
}

/// An open WinHTTP handle, closed on drop.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void) -> io::Result<Self> {
        if raw.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Handle(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by WinHTTP and is closed exactly once.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
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
