use std::time::Duration;

use reqwest::{Client, Method, StatusCode, Url, header::CONTENT_TYPE};
use serde::{Serialize, de::DeserializeOwned};

use crate::RemoteError;

const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) struct Transport {
    client: Client,
    base: Url,
}

impl Transport {
    pub(crate) fn new(endpoint: &str) -> Result<Self, RemoteError> {
        let base = Url::parse(endpoint)
            .map_err(|_| RemoteError::InvalidEndpoint("endpoint must be an absolute URL"))?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(RemoteError::InvalidEndpoint(
                "endpoint must be an HTTP URL with no credentials, path, query, or fragment",
            ));
        }

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|error| RemoteError::Unavailable(error.to_string()))?;
        Ok(Self { client, base })
    }

    pub(crate) fn url(&self, path: &str) -> Url {
        self.base
            .join(path.trim_start_matches('/'))
            .expect("validated endpoint accepts fixed API paths")
    }

    pub(crate) async fn send<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        url: Url,
        body: Option<&B>,
    ) -> Result<T, RemoteError> {
        let retry_write = method != Method::GET && method != Method::HEAD;
        let encoded = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|error| RemoteError::Protocol(format!("invalid request body: {error}")))?;
        let send_once = || {
            let mut request = self.client.request(method.clone(), url.clone());
            if let Some(body) = &encoded {
                request = request
                    .header(CONTENT_TYPE, "application/json")
                    .body(body.clone());
            }
            request.send()
        };
        let attempts = if retry_write { 2 } else { 1 };
        'attempt: for attempt in 0..attempts {
            let mut response = match send_once().await {
                Ok(response) => response,
                Err(_) if attempt + 1 < attempts => continue,
                Err(error) => return Err(RemoteError::Unavailable(error.to_string())),
            };
            let status = response.status();
            let mut bytes = Vec::new();
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        if chunk.len() as u64 > MAX_RESPONSE_BYTES - bytes.len() as u64 {
                            return Err(RemoteError::Protocol(
                                "server response is too large".into(),
                            ));
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    Ok(None) => break,
                    Err(_) if attempt + 1 < attempts => continue 'attempt,
                    Err(error) => return Err(RemoteError::Unavailable(error.to_string())),
                }
            }
            if !status.is_success() {
                return Err(RemoteError::Http {
                    status,
                    body: bytes,
                });
            }
            return serde_json::from_slice(&bytes).map_err(|error| {
                RemoteError::Protocol(format!("invalid server response: {error}"))
            });
        }
        unreachable!("the loop returns after its last attempt")
    }
}

pub(crate) fn is_unavailable_status(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::REQUEST_TIMEOUT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread,
        time::Instant,
    };

    fn accept_with_deadline(listener: &TcpListener) -> Option<TcpStream> {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    return Some(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("listener failed: {error}"),
            }
        }
    }

    fn serve_json(body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let mut stream = accept_with_deadline(&listener).expect("expected one request");
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 1024];
            assert!(stream.read(&mut request).unwrap() > 0);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            let _ = stream.write_all(&body);
        });
        (endpoint, worker)
    }

    #[test]
    fn endpoint_rejects_credentials_and_path_prefixes() {
        assert!(Transport::new("http://user:pass@localhost:8118/").is_err());
        assert!(Transport::new("http://localhost:8118/other").is_err());
    }

    #[tokio::test]
    async fn retry_sends_the_identical_request_after_a_lost_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut bodies = Vec::new();
            for attempt in 0..2 {
                let Some(mut stream) = accept_with_deadline(&listener) else {
                    break;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0_u8; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse().ok())
                    })
                    .unwrap();
                while request.len() < header_end + length {
                    let mut chunk = [0_u8; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                }
                bodies.push(request[header_end..header_end + length].to_vec());
                if attempt == 1 {
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").unwrap();
                }
            }
            bodies
        });
        let transport = Transport::new(&endpoint).unwrap();
        let body = serde_json::json!({"request_id": "stable-id", "task_id": "stable-task"});
        let response: serde_json::Value = transport
            .send(Method::POST, transport.url("v1/tasks"), Some(&body))
            .await
            .unwrap();
        assert_eq!(response, serde_json::json!({"ok": true}));
        let bodies = server.join().unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0], bodies[1]);
    }

    #[tokio::test]
    async fn a_write_stops_after_two_truncated_response_bodies() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..2 {
                let Some(mut stream) = accept_with_deadline(&listener) else {
                    break;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0_u8; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse().ok())
                    })
                    .unwrap();
                while request.len() < header_end + length {
                    let mut chunk = [0_u8; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                }
                requests.push(request[header_end..header_end + length].to_vec());
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\nConnection: close\r\n\r\npart").unwrap();
            }
            requests
        });
        let transport = Transport::new(&endpoint).unwrap();
        let body = serde_json::json!({"request_id": "same-request"});
        let result: Result<serde_json::Value, _> = transport
            .send(
                Method::POST,
                transport.url("v1/tasks/archive-inactive"),
                Some(&body),
            )
            .await;
        assert!(matches!(result, Err(RemoteError::Unavailable(_))));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
    }

    #[tokio::test]
    async fn a_read_with_a_lost_response_is_not_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut first = accept_with_deadline(&listener).expect("expected one read");
            let mut request = [0_u8; 1024];
            assert!(first.read(&mut request).unwrap() > 0);
            drop(first);
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut second, _)) => {
                        second.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue").unwrap();
                        return true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("listener failed: {error}"),
                }
            }
            false
        });
        let transport = Transport::new(&endpoint).unwrap();
        let result: Result<bool, _> = transport
            .send(Method::GET, transport.url("v1/snapshot"), None::<&()>)
            .await;
        assert!(result.is_err());
        assert!(!server.join().unwrap());
    }

    #[tokio::test]
    async fn accepts_a_large_response_within_the_sixteen_megabyte_limit() {
        let payload = format!("\"{}\"", "x".repeat(2_000_000)).into_bytes();
        let (endpoint, worker) = serve_json(payload);
        let transport = Transport::new(&endpoint).unwrap();
        let received: String = transport
            .send(Method::GET, transport.url("v1/snapshot"), None::<&()>)
            .await
            .unwrap();
        assert_eq!(received.len(), 2_000_000);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn rejects_a_response_larger_than_sixteen_megabytes() {
        let payload = format!("\"{}\"", "x".repeat(16 * 1024 * 1024)).into_bytes();
        let (endpoint, worker) = serve_json(payload);
        let transport = Transport::new(&endpoint).unwrap();
        let result: Result<String, _> = transport
            .send(Method::GET, transport.url("v1/snapshot"), None::<&()>)
            .await;
        assert!(
            matches!(result, Err(RemoteError::Protocol(message)) if message == "server response is too large")
        );
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn accepts_a_response_at_the_exact_limit() {
        let payload = format!("\"{}\"", "x".repeat(16 * 1024 * 1024 - 2)).into_bytes();
        let (endpoint, worker) = serve_json(payload);
        let transport = Transport::new(&endpoint).unwrap();
        let received: String = transport
            .send(Method::GET, transport.url("v1/snapshot"), None::<&()>)
            .await
            .unwrap();
        assert_eq!(received.len(), 16 * 1024 * 1024 - 2);
        worker.join().unwrap();
    }

    #[test]
    fn classifies_http_availability_without_treating_bad_requests_as_outages() {
        assert!(is_unavailable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_unavailable_status(StatusCode::REQUEST_TIMEOUT));
        assert!(!is_unavailable_status(StatusCode::BAD_REQUEST));
        assert!(!is_unavailable_status(StatusCode::CONFLICT));
    }
}
