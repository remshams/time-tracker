//! A response gate between a remote TUI and the real HTTP server.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const REQUEST_LIMIT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
pub(crate) enum Route {
    Snapshot,
    CreateTask,
    InactivePreview,
    ArchiveInactive,
    TaskHistory,
}

impl Route {
    fn matches(self, method: &str, path: &str) -> bool {
        match self {
            Self::Snapshot => method == "GET" && path == "/v1/snapshot",
            Self::CreateTask => method == "POST" && path == "/v1/tasks",
            Self::InactivePreview => {
                method == "GET" && path.starts_with("/v1/tasks/inactive-preview?")
            }
            Self::ArchiveInactive => method == "POST" && path == "/v1/tasks/archive-inactive",
            Self::TaskHistory => {
                method == "GET" && path.starts_with("/v1/tasks/") && path.ends_with("/worklogs")
            }
        }
    }
}

#[derive(Default)]
struct GateState {
    route: Option<Route>,
    captured: usize,
    delivered: usize,
    released: bool,
}

struct Shared {
    gate: Mutex<GateState>,
    changed: Condvar,
    stop: AtomicBool,
}

/// Forwards every request to the real server and can hold one route's replies.
pub(crate) struct ControlledProxy {
    address: SocketAddr,
    shared: Arc<Shared>,
    listener: Option<JoinHandle<()>>,
}

impl ControlledProxy {
    pub(crate) fn new(upstream: SocketAddr) -> Self {
        let listener =
            TcpListener::bind(("127.0.0.1", 0)).expect("the response gate must bind to loopback");
        let address = listener
            .local_addr()
            .expect("the gate must have an address");
        listener
            .set_nonblocking(true)
            .expect("the gate listener must be nonblocking");
        let shared = Arc::new(Shared {
            gate: Mutex::new(GateState::default()),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let state = Arc::clone(&shared);
        let handle = thread::spawn(move || {
            let mut workers = Vec::new();
            while !state.stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let state = Arc::clone(&state);
                        workers.push(thread::spawn(move || forward(client, upstream, &state)));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("the gate could not accept a request: {error}"),
                }
            }
            for worker in workers {
                worker.join().expect("a gate request worker must finish");
            }
        });
        Self {
            address,
            shared,
            listener: Some(handle),
        }
    }

    pub(crate) fn endpoint(&self) -> String {
        format!("http://{}", self.address)
    }

    pub(crate) fn hold(&self, route: Route) {
        let mut gate = self.shared.gate.lock().unwrap();
        assert!(gate.route.is_none(), "a response is already held");
        *gate = GateState {
            route: Some(route),
            captured: 0,
            delivered: 0,
            released: false,
        };
    }

    pub(crate) fn wait_for_request(&self) {
        let deadline = Instant::now() + REQUEST_LIMIT;
        let mut gate = self.shared.gate.lock().unwrap();
        while gate.captured == 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "the held HTTP request was never sent");
            (gate, _) = self.shared.changed.wait_timeout(gate, remaining).unwrap();
        }
    }

    pub(crate) fn request_count(&self) -> usize {
        self.shared.gate.lock().unwrap().captured
    }

    pub(crate) fn wait_for_delivery(&self) {
        let deadline = Instant::now() + REQUEST_LIMIT;
        let mut gate = self.shared.gate.lock().unwrap();
        while gate.delivered == 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "the held HTTP response was never delivered"
            );
            (gate, _) = self.shared.changed.wait_timeout(gate, remaining).unwrap();
        }
    }

    pub(crate) fn release(&self) {
        let mut gate = self.shared.gate.lock().unwrap();
        gate.released = true;
        self.shared.changed.notify_all();
    }
}

impl Drop for ControlledProxy {
    fn drop(&mut self) {
        self.release();
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(listener) = self.listener.take() {
            listener
                .join()
                .expect("the response gate must stop cleanly");
        }
    }
}

fn forward(mut client: TcpStream, upstream: SocketAddr, shared: &Shared) {
    client
        .set_nonblocking(false)
        .expect("the accepted gate connection must use blocking reads");
    client
        .set_read_timeout(Some(REQUEST_LIMIT))
        .expect("the gate must set a client timeout");
    let Some(request) = read_request(&mut client) else {
        return;
    };
    let first_line = request.split(|byte| *byte == b'\n').next().unwrap();
    let first_line = std::str::from_utf8(first_line).expect("the HTTP request line must be text");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap();
    let path = parts.next().unwrap();

    let mut server = TcpStream::connect(upstream).expect("the real server must be reachable");
    server
        .set_read_timeout(Some(REQUEST_LIMIT))
        .expect("the gate must set a server timeout");
    let header_end = request
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("the HTTP request must have a complete header");
    let header = std::str::from_utf8(&request[..header_end]).expect("HTTP headers must be text");
    let mut lines = header.split("\r\n");
    let mut forwarded = format!(
        "{}\r\nHost: {upstream}\r\nConnection: close\r\n",
        lines.next().unwrap()
    );
    for line in lines {
        if !line.to_ascii_lowercase().starts_with("host:")
            && !line.to_ascii_lowercase().starts_with("connection:")
        {
            forwarded.push_str(line);
            forwarded.push_str("\r\n");
        }
    }
    forwarded.push_str("\r\n");
    server.write_all(forwarded.as_bytes()).unwrap();
    server.write_all(&request[header_end + 4..]).unwrap();
    let mut response = Vec::new();
    server
        .read_to_end(&mut response)
        .expect("the real server must answer the proxied request");

    let mut gate = shared.gate.lock().unwrap();
    let held = gate.route.is_some_and(|route| route.matches(method, path));
    if held {
        gate.captured += 1;
        shared.changed.notify_all();
        while !gate.released {
            gate = shared.changed.wait(gate).unwrap();
        }
    }
    drop(gate);
    client
        .write_all(&response)
        .expect("the client must accept the released response");
    if held {
        let mut gate = shared.gate.lock().unwrap();
        gate.delivered += 1;
        shared.changed.notify_all();
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut request = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let len = stream
            .read(&mut chunk)
            .expect("the gate must read the request");
        if len == 0 {
            return None;
        }
        request.extend_from_slice(&chunk[..len]);
        if let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let header = std::str::from_utf8(&request[..header_end]).unwrap();
            let body_len = header
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + body_len {
                return Some(request);
            }
        }
    }
}

#[test]
fn abandoned_requests_do_not_reach_the_server_or_count_toward_the_gate() {
    let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let proxy = ControlledProxy::new(upstream.local_addr().unwrap());
    proxy.hold(Route::Snapshot);
    let server = thread::spawn(move || {
        let (mut client, _) = upstream.accept().unwrap();
        client.set_read_timeout(Some(REQUEST_LIMIT)).unwrap();
        let request = read_request(&mut client).unwrap();
        assert!(request.starts_with(b"GET /v1/snapshot HTTP/1.1\r\n"));
        client
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .unwrap();
    });

    for partial in [
        b"".as_slice(),
        b"POST /v1/tasks HTTP/1.1\r\nContent-Length: 5\r\n\r\nab".as_slice(),
    ] {
        let mut client = TcpStream::connect(proxy.address).unwrap();
        client.write_all(partial).unwrap();
    }

    let mut client = TcpStream::connect(proxy.address).unwrap();
    client.set_read_timeout(Some(REQUEST_LIMIT)).unwrap();
    client
        .write_all(b"GET /v1/snapshot HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    proxy.wait_for_request();
    assert_eq!(proxy.request_count(), 1);
    proxy.release();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with("\r\n\r\n{}"));
    proxy.wait_for_delivery();
    server.join().unwrap();
    drop(proxy);
}
