use std::time::Duration;

use reqwest::{Client, Method, StatusCode, Url, header::CONTENT_TYPE};
use serde::{Serialize, de::DeserializeOwned};

use crate::RemoteError;

const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) struct Transport {
    client: Client,
    write_client: Client,
    base: Url,
}

/// A response and whether the request may have reached the server.
pub(crate) struct Delivery<T> {
    pub(crate) result: Result<T, RemoteError>,
    pub(crate) may_have_reached_server: bool,
}

struct SendFailure {
    error: RemoteError,
    may_have_reached_server: bool,
}

impl SendFailure {
    fn after_response(error: RemoteError) -> Self {
        Self {
            error,
            may_have_reached_server: true,
        }
    }
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

        let builder = || {
            Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
        };
        let client = builder()
            .build()
            .map_err(|error| RemoteError::Unavailable(error.to_string()))?;
        // Writes open a new connection each time. A server that closed an idle pooled
        // connection would otherwise fail the write after sending it, which hides whether
        // the server was already gone.
        let write_client = builder()
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|error| RemoteError::Unavailable(error.to_string()))?;
        Ok(Self {
            client,
            write_client,
            base,
        })
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
        self.send_with_delivery(method, url, body).await.result
    }

    /// Sends a request and reports whether any attempt may have reached the server.
    pub(crate) async fn send_with_delivery<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        url: Url,
        body: Option<&B>,
    ) -> Delivery<T> {
        let retry_write = method != Method::GET && method != Method::HEAD;
        let encoded = match body.map(serde_json::to_vec).transpose() {
            Ok(encoded) => encoded,
            Err(error) => {
                return Delivery {
                    result: Err(RemoteError::Protocol(format!(
                        "invalid request body: {error}"
                    ))),
                    may_have_reached_server: false,
                };
            }
        };
        let attempts = if retry_write { 2 } else { 1 };
        let mut may_have_reached_server = false;
        for attempt in 0..attempts {
            let client = if retry_write {
                &self.write_client
            } else {
                &self.client
            };
            let (status, bytes) =
                match Self::send_once(client, &method, &url, encoded.as_deref()).await {
                    Ok(response) => response,
                    Err(failure) => {
                        may_have_reached_server |= failure.may_have_reached_server;
                        if matches!(failure.error, RemoteError::Unavailable(_))
                            && attempt + 1 < attempts
                        {
                            continue;
                        }
                        return Delivery {
                            result: Err(failure.error),
                            may_have_reached_server,
                        };
                    }
                };
            return Delivery {
                result: Self::decode_response(status, bytes),
                may_have_reached_server: true,
            };
        }
        unreachable!("the loop returns after its last attempt")
    }

    fn decode_response<T: DeserializeOwned>(
        status: StatusCode,
        bytes: Vec<u8>,
    ) -> Result<T, RemoteError> {
        if status.is_success() {
            serde_json::from_slice(&bytes)
                .map_err(|error| RemoteError::Protocol(format!("invalid server response: {error}")))
        } else {
            Err(RemoteError::Http {
                status,
                body: bytes,
            })
        }
    }

    async fn send_once(
        client: &Client,
        method: &Method,
        url: &Url,
        body: Option<&[u8]>,
    ) -> Result<(StatusCode, Vec<u8>), SendFailure> {
        let mut request = client.request(method.clone(), url.clone());
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }
        // A failed connection never delivers the request. Any later failure may follow delivery.
        let mut response = request.send().await.map_err(|error| SendFailure {
            may_have_reached_server: !error.is_connect(),
            error: RemoteError::Unavailable(error.to_string()),
        })?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            SendFailure::after_response(RemoteError::Unavailable(error.to_string()))
        })? {
            if chunk.len() as u64 > MAX_RESPONSE_BYTES - bytes.len() as u64 {
                return Err(SendFailure::after_response(RemoteError::Protocol(
                    "server response is too large".into(),
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, bytes))
    }
}

pub(crate) fn is_unavailable_status(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::REQUEST_TIMEOUT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
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

    /// Reads one HTTP request from the stream and returns its body.
    fn read_request_body(stream: &mut TcpStream) -> Vec<u8> {
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
        request[header_end..header_end + length].to_vec()
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
                bodies.push(read_request_body(&mut stream));
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
                requests.push(read_request_body(&mut stream));
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\nConnection: close\r\n\r\npart").unwrap();
            }
            requests
        });
        let transport = Transport::new(&endpoint).unwrap();
        let body = serde_json::json!({"request_id": "same-request"});
        let delivery: Delivery<serde_json::Value> = transport
            .send_with_delivery(
                Method::POST,
                transport.url("v1/tasks/archive-inactive"),
                Some(&body),
            )
            .await;
        assert!(matches!(delivery.result, Err(RemoteError::Unavailable(_))));
        assert!(delivery.may_have_reached_server);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
    }

    fn closed_endpoint() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", listener.local_addr().unwrap())
    }

    #[tokio::test]
    async fn a_write_refused_on_both_attempts_never_reached_the_server() {
        let endpoint = closed_endpoint();
        let transport = Transport::new(&endpoint).unwrap();
        let delivery: Delivery<serde_json::Value> = transport
            .send_with_delivery(
                Method::POST,
                transport.url("v1/tasks"),
                Some(&serde_json::json!({"request_id": "refused"})),
            )
            .await;
        assert!(matches!(delivery.result, Err(RemoteError::Unavailable(_))));
        assert!(!delivery.may_have_reached_server);
    }

    #[tokio::test]
    async fn a_write_that_fails_before_encoding_never_reached_the_server() {
        let transport = Transport::new(&closed_endpoint()).unwrap();
        let body = HashMap::from([((1, 2), 3)]);
        let delivery: Delivery<serde_json::Value> = transport
            .send_with_delivery(Method::POST, transport.url("v1/tasks"), Some(&body))
            .await;
        assert!(
            matches!(delivery.result, Err(RemoteError::Protocol(message)) if message.starts_with("invalid request body"))
        );
        assert!(!delivery.may_have_reached_server);
    }

    #[tokio::test]
    async fn a_write_with_an_error_response_may_have_reached_the_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut stream = accept_with_deadline(&listener).expect("expected one request");
            let mut request = [0_u8; 1024];
            assert!(stream.read(&mut request).unwrap() > 0);
            stream
                .write_all(
                    b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let transport = Transport::new(&endpoint).unwrap();
        let delivery: Delivery<serde_json::Value> = transport
            .send_with_delivery(
                Method::POST,
                transport.url("v1/tasks"),
                Some(&serde_json::json!({"request_id": "answered"})),
            )
            .await;
        assert!(matches!(
            delivery.result,
            Err(RemoteError::Http {
                status: StatusCode::CONFLICT,
                ..
            })
        ));
        assert!(delivery.may_have_reached_server);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn a_write_dropped_mid_request_may_have_reached_the_server_even_if_the_resend_is_refused()
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut stream = accept_with_deadline(&listener).expect("expected one request");
            let mut request = [0_u8; 1024];
            assert!(stream.read(&mut request).unwrap() > 0);
            // Closing the listener first makes the resend fail to connect.
            drop(listener);
            drop(stream);
        });
        let transport = Transport::new(&endpoint).unwrap();
        let delivery: Delivery<serde_json::Value> = transport
            .send_with_delivery(
                Method::POST,
                transport.url("v1/tasks"),
                Some(&serde_json::json!({"request_id": "dropped"})),
            )
            .await;
        server.join().unwrap();
        assert!(matches!(delivery.result, Err(RemoteError::Unavailable(_))));
        assert!(delivery.may_have_reached_server);
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
            .send(Method::GET, transport.url("v1/tasks"), None::<&()>)
            .await;
        assert!(result.is_err());
        assert!(!server.join().unwrap());
    }

    #[tokio::test]
    async fn rejects_a_response_larger_than_sixteen_megabytes() {
        let payload = format!("\"{}\"", "x".repeat(16 * 1024 * 1024)).into_bytes();
        let (endpoint, worker) = serve_json(payload);
        let transport = Transport::new(&endpoint).unwrap();
        let result: Result<String, _> = transport
            .send(Method::GET, transport.url("v1/tasks"), None::<&()>)
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
            .send(Method::GET, transport.url("v1/tasks"), None::<&()>)
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
