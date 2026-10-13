//! Bind-address validation and DNS-rebinding protection.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::process::Output;

use axum::http::{Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub(super) fn validate_bind_address(
    bind: SocketAddr,
    assigned_to_tailscale: impl FnOnce(IpAddr) -> bool,
) -> Result<(), &'static str> {
    if !allowed_bind_address(bind.ip()) {
        return Err("bind must name one localhost or Tailscale address");
    }
    if !bind.ip().is_loopback() && !assigned_to_tailscale(bind.ip()) {
        return Err("bind address is not assigned to this device by Tailscale");
    }
    Ok(())
}

pub(super) fn tailscale_address_matches(
    address: IpAddr,
    lookup: impl FnOnce(&str) -> io::Result<Output>,
) -> bool {
    let family = if address.is_ipv4() { "-4" } else { "-6" };
    let Ok(output) = lookup(family) else {
        return false;
    };
    output.status.success()
        && std::str::from_utf8(&output.stdout)
            .ok()
            .is_some_and(|text| {
                text.lines()
                    .any(|line| line.trim().parse::<IpAddr>() == Ok(address))
            })
}

/// Accepted interface addresses for the unauthenticated first release.
pub fn allowed_bind_address(address: IpAddr) -> bool {
    if address.is_loopback() {
        return true;
    }
    match address {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            octets[0] == 100 && (64..=127).contains(&octets[1])
        }
        IpAddr::V6(ip) => ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

pub(super) async fn reject_unexpected_host(
    request: Request<axum::body::Body>,
    next: Next,
    bind: SocketAddr,
) -> Response {
    let allowed = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| host_allowed(host, bind));
    if allowed {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

fn host_allowed(host: &str, bind: SocketAddr) -> bool {
    let address = bind.ip().to_string();
    let address = if bind.is_ipv6() {
        format!("[{address}]")
    } else {
        address
    };
    host == address
        || host == format!("{address}:{}", bind.port())
        || bind.ip().is_loopback()
            && (host.eq_ignore_ascii_case("localhost")
                || host.eq_ignore_ascii_case(&format!("localhost:{}", bind.port())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router_for_database;
    use axum::body::Body;
    use axum::middleware::from_fn;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use tower::ServiceExt;

    #[test]
    fn accepts_only_explicit_loopback_or_tailscale_addresses() {
        for address in ["127.0.0.1", "::1", "100.100.100.100", "fd7a:115c:a1e0::1"] {
            assert!(allowed_bind_address(address.parse().unwrap()));
        }
        for address in [
            "0.0.0.0",
            "::",
            "192.168.1.2",
            "8.8.8.8",
            "ff02::1",
            "100.63.255.255",
            "100.128.0.1",
            "100.1.0.1",
            "192.100.0.1",
        ] {
            assert!(!allowed_bind_address(address.parse().unwrap()));
        }
        for address in ["100.64.0.1", "100.127.255.254"] {
            assert!(allowed_bind_address(address.parse().unwrap()));
        }
    }

    #[test]
    fn rejects_dns_rebinding_hostnames() {
        let bind = "127.0.0.1:8765".parse().unwrap();
        assert!(host_allowed("127.0.0.1:8765", bind));
        assert!(host_allowed("localhost:8765", bind));
        assert!(!host_allowed("example.test:8765", bind));
        assert!(!host_allowed("127.0.0.1:9999", bind));
        let bind = "[fd7a:115c:a1e0::1]:8765".parse().unwrap();
        assert!(host_allowed("[fd7a:115c:a1e0::1]:8765", bind));
        assert!(!host_allowed("localhost:8765", bind));
    }

    #[tokio::test]
    async fn host_middleware_rejects_missing_and_untrusted_hosts() {
        let directory = tempfile::tempdir().unwrap();
        let bind: SocketAddr = "127.0.0.1:8765".parse().unwrap();
        let router = router_for_database(&directory.path().join("host.db"))
            .unwrap()
            .layer(from_fn(move |request, next| {
                reject_unexpected_host(request, next, bind)
            }));

        let allowed = Request::builder()
            .uri("/v1/health")
            .header("host", "localhost:8765")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(allowed).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let untrusted = Request::builder()
            .uri("/v1/health")
            .header("host", "tracker.attacker.test:8765")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(untrusted).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let missing = Request::builder()
            .uri("/v1/health")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(missing).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn bind_validation_skips_tailscale_lookup_for_loopback_and_requires_it_elsewhere() {
        let loopback: SocketAddr = "127.0.0.1:8765".parse().unwrap();
        let mut lookup_called = false;
        assert!(
            validate_bind_address(loopback, |_| {
                lookup_called = true;
                false
            })
            .is_ok()
        );
        assert!(!lookup_called, "loopback does not need a Tailscale lookup");

        let tailscale_address: SocketAddr = "100.100.100.100:8765".parse().unwrap();
        assert!(
            validate_bind_address(tailscale_address, |address| {
                address == tailscale_address.ip()
            })
            .is_ok()
        );
        assert!(
            validate_bind_address(tailscale_address, |_| false)
                .unwrap_err()
                .contains("not assigned")
        );

        let ordinary_lan: SocketAddr = "192.168.1.20:8765".parse().unwrap();
        assert!(
            validate_bind_address(ordinary_lan, |_| {
                panic!("invalid bind addresses must fail before checking Tailscale")
            })
            .unwrap_err()
            .contains("localhost or Tailscale")
        );
    }

    #[test]
    fn tailscale_lookup_checks_family_output_and_success_status() {
        assert!(tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |family| {
                assert_eq!(family, "-4");
                Ok(lookup_output(0, b"100.101.102.103\n"))
            }
        ));
        assert!(tailscale_address_matches(
            "fd7a:115c:a1e0::42".parse().unwrap(),
            |family| {
                assert_eq!(family, "-6");
                Ok(lookup_output(0, b"fd7a:115c:a1e0::42\n"))
            }
        ));
        assert!(!tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |_| Ok(lookup_output(0, b"100.101.102.104\n"))
        ));
        assert!(!tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |_| Ok(lookup_output(1, b"100.101.102.103\n"))
        ));
        assert!(!tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |_| Ok(lookup_output(0, b"\xff"))
        ));
        assert!(!tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        ));
    }

    fn lookup_output(status: i32, stdout: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(status << 8),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }
}
