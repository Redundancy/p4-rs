//! Integration tests that exercise the wrapper against a real `p4d`.
//!
//! The server-backed tests are `#[ignore]`d so a plain `cargo test` stays green
//! without a server; run them with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test -- --ignored
//! ```
//!
//! If `P4D_BIN` is not set, each ignored test skips itself with a note rather
//! than failing, so `-- --ignored` is safe to run anywhere. CI downloads `p4d`
//! from Perforce filehost (same release directory as the SDK) and points
//! `P4D_BIN` at it. An unlicensed p4d allows 5 users / 20 workspaces -- ample.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use p4_rs::client;
use p4_rs::commands::client::ViewMapping;
use p4_rs::commands::{info, users};

/// A throwaway `p4d` server rooted in a temp directory, killed on drop.
struct TestServer {
    child: Child,
    port: String,
    root: PathBuf,
}

impl TestServer {
    /// Start `p4d` on a free localhost port, or return `None` if `P4D_BIN` is
    /// not set (the signal to skip the test).
    fn start(name: &str) -> Option<TestServer> {
        let p4d = std::env::var("P4D_BIN").ok()?;

        let port = format!("localhost:{}", free_port());

        // Unique temp root per test so parallel tests don't collide.
        let root = std::env::temp_dir().join(format!("p4-rs-it-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&root).expect("create p4d root");

        let child = Command::new(&p4d)
            .arg("-r")
            .arg(&root)
            .arg("-p")
            .arg(&port)
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn p4d ({p4d}): {e}"));

        let server = TestServer { child, port, root };
        server.wait_until_ready();
        Some(server)
    }

    fn wait_until_ready(&self) {
        let addr = self.port.replace("localhost", "127.0.0.1");
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if TcpStream::connect(&addr).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "p4d did not start listening on {} within timeout",
            self.port
        );
    }

    fn connect(&self) -> client::Client {
        client::Options::new()
            .set_program("p4-rs-integration-test")
            .set_port(&self.port)
            .connect()
            .expect("connect to local p4d")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Ask the OS for a free TCP port by binding to :0 and releasing it. There is
/// a small race between release and p4d re-binding, but it is fine for tests.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

fn skip(name: &str) {
    let _ = writeln!(
        std::io::stderr(),
        "skipping {name}: set P4D_BIN to a p4d executable to run integration tests"
    );
}

/// Connecting to a port nothing listens on must surface an Err from Init, not
/// hang, panic, or misreport success. Needs no p4d, so it always runs.
#[test]
fn connect_to_dead_port_fails() {
    let port = format!("localhost:{}", free_port());
    let result = client::Options::new()
        .set_program("p4-rs-integration-test")
        .set_port(&port)
        .connect();
    assert!(result.is_err(), "connect to {port} should fail");
}

#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn info_roundtrip() {
    let Some(server) = TestServer::start("info") else {
        skip("info_roundtrip");
        return;
    };

    let mut c = server.connect();
    let r = c
        .info(&info::Options::new().shortened())
        .expect("typed info against a live p4d");

    assert!(!r.user_name.is_empty(), "user name should be populated");
    assert!(
        !r.server_version.is_empty(),
        "server version should be populated"
    );
}

#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn run_records_on_multi_record_command() {
    let Some(server) = TestServer::start("records") else {
        skip("run_records_on_multi_record_command");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();
    // A fresh server has no changelists: the command succeeds with 0 records
    // (as opposed to erroring or fabricating output).
    let records = c
        .run_records(&mut ui, "changes", Vec::new())
        .expect("changes against a live p4d");
    assert!(records.is_empty(), "fresh server should have no changes");
}

/// The create -> modify -> save -> re-read cycle for client specs, all typed.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn clientspec_create_modify_reread() {
    let Some(server) = TestServer::start("clientspec") else {
        skip("clientspec_create_modify_reread");
        return;
    };

    let mut c = server.connect();

    // A spec for a client that doesn't exist yet is a defaulted template.
    let mut spec = c.client_spec(Some("it-ws")).expect("read spec template");
    assert_eq!(spec.client, "it-ws");
    assert!(
        spec.update.is_none(),
        "unsaved template has no Update stamp"
    );

    // Modify: description, root, and an added view exclusion.
    spec.description = "Integration test workspace.".to_string();
    spec.root = server.root.join("ws").to_string_lossy().into_owned();
    spec.view.push(ViewMapping::new(
        "-//depot/excluded/...",
        "//it-ws/excluded/...",
    ));
    spec.options.clobber = true;

    c.save_client_spec(&spec).expect("save modified spec");

    // Re-read: our modifications persisted and the server stamped it.
    let saved = c.client_spec(Some("it-ws")).expect("re-read saved spec");
    assert_eq!(saved.description.trim_end(), "Integration test workspace.");
    assert!(saved.options.clobber);
    assert!(saved.update.is_some(), "saved spec has an Update stamp");
    assert_eq!(saved.view.len(), 2);
    assert_eq!(saved.view[1].depot, "-//depot/excluded/...");
}

/// users() lists accounts, typed -- after creating our own user record via
/// `user -i` (which also exercises the InputData plumbing end to end).
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn users_lists_created_user() {
    let Some(server) = TestServer::start("users") else {
        skip("users_lists_created_user");
        return;
    };

    let mut c = server.connect();

    // Whoever we're connected as (OS-dependent) is who we can create. Note:
    // info()'s userName is literally "*unknown*" until the user record exists
    // (at least on 2022.2), so read the name from the user -o template, which
    // reports the client-resolved name.
    let mut ui = client::UserInterface::new();
    let template = c
        .run_records(&mut ui, "user", vec!["-o".to_string()])
        .expect("user -o template");
    let me = template
        .first()
        .and_then(|r| r.get("User"))
        .expect("template User field")
        .clone();

    ui.set_input(&format!(
        "User:\t{me}\n\nEmail:\t{me}@example.test\n\nFullName:\tIntegration Test\n"
    ));
    c.run_records(&mut ui, "user", vec!["-i".to_string()])
        .expect("save own user spec");

    let listed = c.users(&users::Options::new()).expect("list users");
    let mine = listed
        .iter()
        .find(|u| u.user == me)
        .expect("created user should be listed");
    assert_eq!(mine.email, format!("{me}@example.test"));
    assert_eq!(mine.full_name, "Integration Test");
    assert!(
        mine.update > 0,
        "tagged Update should be an epoch timestamp"
    );
}

#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn unknown_command_returns_err() {
    let Some(server) = TestServer::start("badcmd") else {
        skip("unknown_command_returns_err");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();
    // Exercises the error path end-to-end: the server rejects the command, the
    // ClientUser accumulates the error, and run() must return Err -- not
    // silently succeed with empty output.
    let result = c.run(&mut ui, "this-is-not-a-real-p4-command", Vec::new());
    assert!(result.is_err(), "unknown command should be an error");
}
