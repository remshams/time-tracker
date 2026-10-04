//! Real server process and remote TUI fixtures.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::context::TestContext;
use crate::controlled_proxy::ControlledProxy;

const SERVER_START_LIMIT: Duration = Duration::from_secs(5);
const HEALTH_REQUEST: &[u8] =
    b"GET /v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";

/// One isolated server database and one or more real remote TUI processes.
pub(crate) struct RemoteTestContext {
    local: TestContext,
    database_path: std::path::PathBuf,
    address: SocketAddr,
    server: Option<Child>,
}

impl RemoteTestContext {
    pub(crate) fn new() -> Self {
        let mut context = Self::new_without_server();
        context.start_server();
        context
    }

    pub(crate) fn new_with_tasks() -> Self {
        let mut context = Self::new_without_server();
        context.server_database().create_fixture_tasks();
        context.start_server();
        context
    }

    pub(crate) fn new_without_server() -> Self {
        let local = TestContext::new();
        let database_path = local.home().join("remote-server.db");
        let address = unused_loopback_address();
        Self {
            local,
            database_path,
            address,
            server: None,
        }
    }

    pub(crate) fn endpoint(&self) -> String {
        format!("http://{}", self.address)
    }

    pub(crate) fn launch(&self) -> crate::driver::TuiDriver {
        self.local.launch_remote(&self.endpoint())
    }

    pub(crate) fn launch_second_client(&self) -> crate::driver::TuiDriver {
        self.local.launch_remote(&self.endpoint())
    }

    pub(crate) fn proxy(&self) -> ControlledProxy {
        ControlledProxy::new(self.address)
    }

    pub(crate) fn launch_through(&self, proxy: &ControlledProxy) -> crate::driver::TuiDriver {
        self.local.launch_remote(&proxy.endpoint())
    }

    pub(crate) fn server_database(&self) -> crate::database::Database {
        crate::database::Database::open(&self.database_path)
    }

    pub(crate) fn local_database_path(&self) -> &std::path::Path {
        self.local.local_database_path()
    }

    pub(crate) fn stop_server(&mut self) {
        if let Some(mut server) = self.server.take() {
            let _ = server.kill();
            let _ = server.wait();
        }
    }

    pub(crate) fn restart_server(&mut self) {
        self.stop_server();
        self.start_server();
    }

    pub(crate) fn start(&mut self) {
        assert!(self.server.is_none(), "the test server is already running");
        self.start_server();
    }

    fn start_server(&mut self) {
        let child = Command::new(env!("CARGO_BIN_EXE_tt"))
            .arg("serve")
            .arg("--bind")
            .arg(self.address.to_string())
            .arg("--db")
            .arg(&self.database_path)
            .env_clear()
            .env("HOME", self.local.home())
            .env("TZ", "UTC")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the real tt server must spawn");
        self.server = Some(child);
        self.wait_until_ready();
    }

    fn wait_until_ready(&mut self) {
        let deadline = Instant::now() + SERVER_START_LIMIT;
        loop {
            if let Some(server) = self.server.as_mut()
                && let Some(status) = server.try_wait().expect("server status must be readable")
            {
                panic!("tt server exited before readiness with {status}");
            }
            if health_check(self.address) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "tt server did not answer /v1/health"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for RemoteTestContext {
    fn drop(&mut self) {
        self.stop_server();
    }
}

fn unused_loopback_address() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .expect("the test must be able to reserve a loopback port");
    listener
        .local_addr()
        .expect("the loopback listener must expose its address")
}

fn health_check(address: SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(50)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    if std::io::Write::write_all(&mut stream, HEALTH_REQUEST).is_err() {
        return false;
    }
    let mut response = [0; 128];
    let Ok(read) = std::io::Read::read(&mut stream, &mut response) else {
        return false;
    };
    response[..read].starts_with(b"HTTP/1.1 200") || response[..read].starts_with(b"HTTP/1.0 200")
}
