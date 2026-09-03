// SPDX-License-Identifier: EUPL-1.2
//! The network between two brokers: the proxy an organization runs in front
//! of its own, and the endpoint a notification is delivered to.
//!
//! A proxy relays bytes, records what it passed on, and can change the
//! request on the way — which is what a real deployment's egress and ingress
//! proxies can do, so it is what a test claiming a property "survives the
//! proxies" has to model.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// Everything a relay passed on, head by head, one entry per request.
pub type Wire = Arc<Mutex<Vec<String>>>;

/// The request as it left the relay, unchanged.
pub fn verbatim(req: String) -> String {
    req
}

/// A relay that deletes `scopeQ` from the request line it passes on.
pub fn strip_scope_q(req: String) -> String {
    rewrite_query(req, |p| !p.starts_with("scopeQ="))
}

/// Rebuild the request line's query string, keeping the parameters `keep`
/// accepts. Splitting the whole line on `&` instead would swallow the
/// ` HTTP/1.1` that follows the last parameter.
fn rewrite_query(req: String, keep: fn(&str) -> bool) -> String {
    let Some((line, rest)) = req.split_once("\r\n") else {
        return req;
    };
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    let [method, target, version] = parts[..] else {
        return req;
    };
    let target = match target.split_once('?') {
        None => target.to_owned(),
        Some((path, query)) => {
            let kept: Vec<&str> = query.split('&').filter(|p| keep(p)).collect();
            if kept.is_empty() {
                path.to_owned()
            } else {
                format!("{path}?{}", kept.join("&"))
            }
        }
    };
    format!("{method} {target} {version}\r\n{rest}")
}

/// Listen on an ephemeral port and relay every connection to `upstream`,
/// passing the request head through `rewrite` first. Returns the port to
/// point a broker at, and the record of what was relayed.
pub fn proxy(upstream: u16, rewrite: fn(String) -> String) -> (u16, Wire) {
    let seen: Wire = Arc::default();
    let recorder = Arc::clone(&seen);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(down) = stream else { continue };
            let recorder = Arc::clone(&recorder);
            std::thread::spawn(move || one(down, upstream, rewrite, &recorder));
        }
    });
    (port, seen)
}

/// One relayed connection: read the head, rewrite it, then let the two
/// sockets copy into each other so a body and the response travel too.
fn one(mut down: std::net::TcpStream, upstream: u16, rewrite: fn(String) -> String, seen: &Wire) {
    let mut head = Vec::new();
    let mut buf = [0u8; 8192];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match down.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
            Err(_) => return,
        }
    }
    // Whatever arrived past the head is the start of the body; it is not
    // the relay's to interpret, so it goes on untouched behind the head.
    let cut = head
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("head terminator")
        + 4;
    let mut body = head.split_off(cut);
    let relayed = rewrite(String::from_utf8_lossy(&head).into_owned());
    // The recorded entry is the whole request, so a test can read the
    // document a broker sent and not only the line it sent it on. A body
    // that has not all arrived yet is read to its declared length first;
    // without that the record holds whatever one TCP read happened to
    // carry, which is the head alone often enough to look like a bug in
    // the broker.
    if let Some(len) = content_length(&relayed) {
        while body.len() < len {
            match down.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => body.extend_from_slice(&buf[..n]),
                Err(_) => return,
            }
        }
    }
    if let Ok(mut v) = seen.lock() {
        let mut whole = relayed.clone();
        whole.push_str(&String::from_utf8_lossy(&body));
        v.push(whole);
    }
    let Ok(mut up) = std::net::TcpStream::connect(("127.0.0.1", upstream)) else {
        return;
    };
    if up.write_all(relayed.as_bytes()).is_err() || up.write_all(&body).is_err() {
        return;
    }
    let (Ok(mut up_read), Ok(mut down_read)) = (up.try_clone(), down.try_clone()) else {
        return;
    };
    let back = std::thread::spawn(move || {
        let _ = std::io::copy(&mut up_read, &mut down);
    });
    let _ = std::io::copy(&mut down_read, &mut up);
    let _ = back.join();
}

/// A notification endpoint: answers 204 and keeps every body it was posted,
/// one entry per delivery.
pub fn sink() -> (u16, Wire) {
    let seen: Wire = Arc::default();
    let recorder = Arc::clone(&seen);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 65536];
            let n = s.read(&mut buf).unwrap_or(0);
            if let Ok(mut v) = recorder.lock() {
                v.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            }
            let _ = s.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
        }
    });
    (port, seen)
}

/// The `Content-Length` a request head declares, if it declares one.
fn content_length(head: &str) -> Option<usize> {
    head.lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split_once(':'))
        .and_then(|(_, v)| v.trim().parse().ok())
}
