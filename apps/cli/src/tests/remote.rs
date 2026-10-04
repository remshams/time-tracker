use std::net::SocketAddr;
use std::sync::mpsc;
use std::thread;

use clap::Parser;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::{Cli, run};

struct Server {
    _directory: tempfile::TempDir,
    endpoint: String,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("server.db");
        let (address_tx, address_rx) = mpsc::channel();
        let (stop, stopped) = oneshot::channel();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async move {
                let app = tracker_server::router_for_database(&database).unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                address_tx.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await
                    .unwrap();
            });
        });
        let address: SocketAddr = address_rx.recv().unwrap();
        Self {
            _directory: directory,
            endpoint: format!("http://{address}"),
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    fn command(&self, at: &str, arguments: &[&str]) -> Result<Value, crate::CliError> {
        let cli = Cli::try_parse_from(
            ["tt-cli", "--server", self.endpoint.as_str(), "--at", at]
                .into_iter()
                .chain(arguments.iter().copied()),
        )
        .unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(run(cli))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn remote_commands_use_the_server_and_preserve_guards_and_committed_values() {
    let server = Server::start();
    let now = chrono::Utc::now();
    let t0 = (now - chrono::Duration::hours(3)).to_rfc3339();
    let t1 = (now - chrono::Duration::hours(2)).to_rfc3339();
    let t2 = (now - chrono::Duration::hours(1)).to_rfc3339();
    let t3 = now.to_rfc3339();
    let first = server.command(&t0, &["tasks", "create", "First"]).unwrap();
    let second = server.command(&t0, &["tasks", "create", "Second"]).unwrap();
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();
    assert_eq!(
        server.command(&t0, &["tasks", "get", first_id]).unwrap(),
        first
    );
    assert_eq!(
        server
            .command(&t0, &["tasks", "rename", first_id, "First renamed"])
            .unwrap()["name"],
        "First renamed"
    );
    assert_eq!(
        server
            .command(&t0, &["tasks", "list", "--search", "frn"])
            .unwrap()["tasks"][0]["task"]["id"],
        first_id
    );
    let started = server
        .command(&t1, &["tracking", "start", first_id])
        .unwrap();
    let id = started["worklog"]["id"].as_str().unwrap();
    assert_eq!(
        server
            .command(&t1, &["tracking", "start", first_id])
            .unwrap()["outcome"],
        "already_active"
    );
    assert_eq!(
        server.command(&t2, &["tracking", "status"]).unwrap()["active_worklog"]["id"],
        id
    );
    server
        .command(&t2, &["tracking", "stop", "--expected-active", id])
        .unwrap();
    let worklog = server
        .command(&t3, &["worklogs", "list", "--task", first_id])
        .unwrap()["worklogs"][0]
        .clone();
    let start = worklog["start"].as_str().unwrap();
    let end = worklog["end"].as_str().unwrap();
    let corrected = server
        .command(
            &t3,
            &[
                "worklogs",
                "correct",
                id,
                "--expected-start",
                start,
                "--expected-end",
                end,
                "--start",
                &t0,
            ],
        )
        .unwrap();
    assert_eq!(corrected["end"], end);
    assert_eq!(
        server
            .command(
                &t3,
                &[
                    "worklogs",
                    "move",
                    id,
                    second_id,
                    "--expected-task",
                    first_id,
                    "--expected-start",
                    start,
                    "--expected-end",
                    end
                ]
            )
            .unwrap_err()
            .code,
        "worklog_changed"
    );
    let start = corrected["start"].as_str().unwrap();
    let moved = server
        .command(
            &t3,
            &[
                "worklogs",
                "move",
                id,
                second_id,
                "--expected-task",
                first_id,
                "--expected-start",
                start,
                "--expected-end",
                end,
            ],
        )
        .unwrap();
    assert_eq!(moved["task_id"], second_id);
    let report = server
        .command(
            &t3,
            &["reports", "--start", &t0, "--end", &t3, "--timezone", "UTC"],
        )
        .unwrap();
    assert_eq!(report["rows"][0]["task"]["id"], second_id);
    assert!(report["total_us"].as_i64().unwrap() > 0);
    assert_eq!(
        server.command(&t3, &["worklogs", "list"]).unwrap()["worklogs"][0],
        moved
    );
    assert_eq!(
        server
            .command(
                &t3,
                &[
                    "worklogs",
                    "delete",
                    id,
                    "--expected-task",
                    second_id,
                    "--expected-start",
                    start,
                    "--expected-end",
                    end,
                    "--yes"
                ]
            )
            .unwrap()["deleted"],
        moved
    );
    assert_eq!(
        server
            .command(&t3, &["tasks", "archive", second_id])
            .unwrap()["archived"],
        true
    );
    assert_eq!(
        server
            .command(&t3, &["tasks", "restore", second_id])
            .unwrap()["archived"],
        false
    );
    let preview = server.command(&t3, &["tasks", "preview-inactive"]).unwrap();
    assert_eq!(preview["count"], 0);
    assert_eq!(
        server
            .command(
                &t3,
                &[
                    "tasks",
                    "archive-inactive",
                    "--preview",
                    &preview["preview"].to_string(),
                    "--yes"
                ]
            )
            .unwrap()["archived_count"],
        0
    );
}
