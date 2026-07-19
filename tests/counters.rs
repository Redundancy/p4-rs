//! Integration tests for the typed counter commands, exercised against a real
//! `p4d`. Like the other server-backed tests these are `#[ignore]`d so a plain
//! `cargo test` stays green without a server; run them with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test counters -- --ignored
//! ```
//!
//! If `P4D_BIN` is not set each test skips itself with a note rather than
//! failing.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use p4_rs::client;

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

        let root =
            std::env::temp_dir().join(format!("p4-rs-counters-it-{}-{}", name, std::process::id()));
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

/// Ask the OS for a free TCP port by binding to :0 and releasing it.
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

/// Capture the raw tagged records the server emits, to shape the typed wrapper
/// against reality. Gated on `P4RS_CAPTURE` so it does not run in the normal
/// `-- --ignored` sweep. Run with:
///
/// ```text
/// P4RS_CAPTURE=1 P4D_BIN=/path/to/p4d cargo test --test counters -- --ignored --nocapture
/// ```
#[test]
#[ignore = "capture-only: set P4RS_CAPTURE=1 and P4D_BIN to dump raw records"]
fn capture_counter_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        skip("capture_counter_records");
        return;
    }
    let Some(server) = TestServer::start("capture") else {
        skip("capture_counter_records");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();

    let set = c
        .run_records(
            &mut ui,
            "counter",
            vec!["p4rs-cap".to_string(), "42".to_string()],
        )
        .expect("set counter p4rs-cap");
    eprintln!("=== counter p4rs-cap 42 (set) ===\n{set:#?}");

    let counters = c
        .run_records(&mut ui, "counters", Vec::new())
        .expect("counters");
    eprintln!("=== counters ===\n{counters:#?}");

    let get = c
        .run_records(&mut ui, "counter", vec!["p4rs-cap".to_string()])
        .expect("counter p4rs-cap (get)");
    eprintln!("=== counter p4rs-cap (get) ===\n{get:#?}");

    let missing = c
        .run_records(&mut ui, "counter", vec!["nonexistent-counter".to_string()])
        .expect("counter nonexistent-counter");
    eprintln!("=== counter nonexistent-counter (get) ===\n{missing:#?}");
}

/// Full lifecycle: set -> get -> list (typed) -> update -> delete -> absent.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test counters -- --ignored`"]
fn counter_roundtrip() {
    let Some(server) = TestServer::start("roundtrip") else {
        skip("counter_roundtrip");
        return;
    };

    let mut c = server.connect();
    let name = "p4rs-test-counter";

    // set + get
    c.set_counter(name, "42").expect("set counter to 42");
    assert_eq!(c.counter(name).expect("get counter"), "42");

    // the typed list contains it
    let listed = c.counters().expect("list counters");
    let found = listed
        .iter()
        .find(|c| c.name == name)
        .expect("counter present in typed listing");
    assert_eq!(found.value, "42");
    assert_eq!(found.as_u64(), Some(42));

    // update
    c.set_counter(name, "43").expect("update counter to 43");
    assert_eq!(c.counter(name).expect("get updated counter"), "43");

    // delete -> reading an absent counter yields "0" and the listing drops it
    c.delete_counter(name).expect("delete counter");
    assert_eq!(
        c.counter(name).expect("get deleted counter"),
        "0",
        "absent counter should read as 0 by p4 convention"
    );
    let listed = c.counters().expect("list counters after delete");
    assert!(
        !listed.iter().any(|c| c.name == name),
        "deleted counter should no longer be listed"
    );
}
