//! Integration tests for the typed `labels` / `label` wrappers against a real
//! `p4d`.
//!
//! Server-backed tests are `#[ignore]`d so a plain `cargo test` stays green
//! without a server; run them with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test label -- --ignored
//! ```
//!
//! If `P4D_BIN` is not set, each ignored test skips itself with a note rather
//! than failing. This file carries its own small `p4d` harness (rather than a
//! shared `tests/common`) so it stands alone.
//!
//! Because the FFI bridge does not yet expose a form-input channel, the
//! library cannot itself save a label (`label -i`); these tests exercise the
//! read paths (`labels`, `label -o`) end to end, using the `p4` command-line
//! client -- shipped alongside `p4d` -- as a fixture to create a label when
//! one is available.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use p4_rs::client;

/// A throwaway `p4d` server rooted in a temp directory, killed on drop.
struct TestServer {
    child: Child,
    port: String,
    root: PathBuf,
    p4d: String,
}

impl TestServer {
    /// Start `p4d` on a free localhost port, or return `None` if `P4D_BIN` is
    /// not set (the signal to skip the test).
    fn start(name: &str) -> Option<TestServer> {
        let p4d = std::env::var("P4D_BIN").ok()?;

        let port = format!("localhost:{}", free_port());
        let root = std::env::temp_dir().join(format!("p4-rs-lbl-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&root).expect("create p4d root");

        let child = Command::new(&p4d)
            .arg("-r")
            .arg(&root)
            .arg("-p")
            .arg(&port)
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn p4d ({p4d}): {e}"));

        let server = TestServer {
            child,
            port,
            root,
            p4d,
        };
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
            .set_program("p4-rs-label-test")
            .set_port(&self.port)
            .set_user("labeltester")
            .connect()
            .expect("connect to local p4d")
    }

    /// Path to the `p4` command-line client, assumed to sit next to `p4d`
    /// (as it does in a Perforce release directory).
    fn p4_client(&self) -> PathBuf {
        let dir = Path::new(&self.p4d)
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let name = if cfg!(windows) { "p4.exe" } else { "p4" };
        dir.join(name)
    }

    /// Create a label by feeding a form to `p4 label -i`. Returns `false` if
    /// the `p4` client is not available beside `p4d`.
    fn create_label_via_cli(&self, form: &str) -> bool {
        let p4 = self.p4_client();
        if !p4.exists() {
            return false;
        }
        let mut child = Command::new(&p4)
            .arg("-p")
            .arg(&self.port)
            .arg("-u")
            .arg("labeltester")
            .arg("label")
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn p4 label -i");
        child
            .stdin
            .take()
            .expect("child stdin")
            .write_all(form.as_bytes())
            .expect("write label form");
        let status = child.wait().expect("wait for p4 label -i");
        assert!(status.success(), "p4 label -i failed: {status}");
        true
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

/// The form fed to `p4 label -i` to create the fixture label.
const REL_LABEL_FORM: &str = "Label:\trel-1.0\n\
     Owner:\tlabeltester\n\
     Description:\tRelease 1.0 label\n\
     Options:\tlocked noautoreload\n\
     View:\n\t//depot/rel/...\n";

/// `labels` on a fresh server returns an empty list -- the command succeeds
/// with zero records rather than erroring.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn labels_empty_on_fresh_server() {
    let Some(server) = TestServer::start("empty") else {
        skip("labels_empty_on_fresh_server");
        return;
    };

    let mut c = server.connect();
    let labels = c.labels().expect("labels against a live p4d");
    assert!(labels.is_empty(), "fresh server should have no labels");
}

/// `label -o <name>` for a name that does not exist returns a default
/// template: the name echoes back, options default to unlocked, and there is
/// no server-managed `Update` yet.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn label_spec_template_for_new_name() {
    let Some(server) = TestServer::start("template") else {
        skip("label_spec_template_for_new_name");
        return;
    };

    let mut c = server.connect();
    let spec = c.label_spec("rel-1.0").expect("label -o template");

    assert_eq!(spec.label, "rel-1.0");
    assert!(spec.update.is_none(), "unsaved template has no Update");
    assert!(!spec.options.locked, "template defaults to unlocked");
    assert!(
        !spec.view.is_empty(),
        "template carries a default view line"
    );
}

/// Full read round-trip: create a label with the `p4` CLI, then verify the
/// library lists it (`labels`) and re-reads its form (`label -o`) with the
/// fields, view, and options intact.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn label_read_roundtrip() {
    let Some(server) = TestServer::start("roundtrip") else {
        skip("label_read_roundtrip");
        return;
    };

    if !server.create_label_via_cli(REL_LABEL_FORM) {
        let _ = writeln!(
            std::io::stderr(),
            "skipping label_read_roundtrip: no `p4` client found beside p4d"
        );
        return;
    }

    let mut c = server.connect();

    // labels() lists the newly-created label with an epoch Update > 0.
    let labels = c.labels().expect("labels after create");
    assert_eq!(labels.len(), 1, "one label should be listed");
    let summary = &labels[0];
    assert_eq!(summary.name, "rel-1.0");
    assert!(summary.update > 0, "list Update is epoch seconds > 0");
    assert!(summary.options.locked, "label was created locked");
    assert_eq!(summary.owner, "labeltester");

    // label_spec() re-reads the saved form with everything round-tripped.
    let spec = c.label_spec("rel-1.0").expect("label -o after create");
    assert_eq!(spec.label, "rel-1.0");
    assert_eq!(
        spec.description.as_deref().map(str::trim_end),
        Some("Release 1.0 label")
    );
    assert_eq!(spec.view, vec!["//depot/rel/...".to_string()]);
    assert!(spec.options.locked, "saved label is locked");
    assert!(
        spec.update.is_some(),
        "a saved label has a server-managed Update timestamp"
    );
}

/// Capture harness: dump the raw records behind `labels` and `label -o`
/// (template and saved) so the typed structs can be shaped to reality. Gated
/// on `P4RS_CAPTURE` so it is inert during ordinary `--ignored` runs.
#[test]
#[ignore = "capture-only; run with P4RS_CAPTURE=1 cargo test --test label -- --ignored --nocapture"]
fn capture_label_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        return;
    }
    let Some(server) = TestServer::start("capture") else {
        skip("capture_label_records");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();

    eprintln!("=== label -o rel-1.0 (TEMPLATE) ===");
    dump(
        &c.run_records(&mut ui, "label", vec!["-o".into(), "rel-1.0".into()])
            .expect("template"),
    );

    let created = server.create_label_via_cli(REL_LABEL_FORM);
    eprintln!("=== created label via CLI: {created} ===");

    let mut ui2 = client::UserInterface::new();
    eprintln!("=== labels ===");
    dump(&c.run_records(&mut ui2, "labels", vec![]).expect("labels"));

    let mut ui3 = client::UserInterface::new();
    eprintln!("=== label -o rel-1.0 (SAVED) ===");
    dump(
        &c.run_records(&mut ui3, "label", vec!["-o".into(), "rel-1.0".into()])
            .expect("saved"),
    );
}

fn dump(records: &[std::collections::HashMap<String, String>]) {
    for (i, r) in records.iter().enumerate() {
        let mut keys: Vec<_> = r.iter().collect();
        keys.sort();
        eprintln!("  record {i}:");
        for (k, v) in keys {
            eprintln!("    [{k}] = {v:?}");
        }
    }
}
