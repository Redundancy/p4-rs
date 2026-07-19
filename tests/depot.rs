//! Integration tests for the typed `depots` / `depot` commands against a real
//! `p4d`.
//!
//! Server-backed tests are `#[ignore]`d so a plain `cargo test` stays green
//! without a server; run them with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test depot -- --ignored
//! ```
//!
//! If `P4D_BIN` is unset each ignored test skips itself with a note rather than
//! failing. This file carries its own throwaway-server harness rather than
//! sharing one, so it builds standalone.

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use p4_rs::client;
use p4_rs::commands::depot::DepotType;

/// A throwaway `p4d` server rooted in a temp directory, killed on drop.
struct TestServer {
    child: Child,
    port: String,
    root: PathBuf,
}

impl TestServer {
    /// Start `p4d` on a free localhost port, or `None` if `P4D_BIN` is unset
    /// (the signal to skip the test).
    fn start(name: &str) -> Option<TestServer> {
        let p4d = std::env::var("P4D_BIN").ok()?;
        let port = format!("localhost:{}", free_port());

        let root =
            std::env::temp_dir().join(format!("p4-rs-depot-{}-{}", name, std::process::id()));
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
            .set_program("p4-rs-depot-test")
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

/// Pretty-print raw tagged records to stderr, key by key, for shaping structs.
fn dump_records(label: &str, records: &[HashMap<String, String>]) {
    let mut err = std::io::stderr();
    let _ = writeln!(err, "\n===== {label} ({} record(s)) =====", records.len());
    for (i, rec) in records.iter().enumerate() {
        let mut keys: Vec<&String> = rec.keys().collect();
        keys.sort();
        let _ = writeln!(err, "  --- record {i} ---");
        for k in keys {
            let _ = writeln!(err, "    {k:>16} = {:?}", rec[k]);
        }
    }
}

/// Capture the raw records behind `depots` and `depot -o` so the typed structs
/// can be shaped to the server's actual keys. Not an assertion; gated on
/// `P4RS_CAPTURE` so it only runs when explicitly requested:
///
/// ```text
/// P4RS_CAPTURE=1 P4D_BIN=/path/to/p4d cargo test --test depot -- --ignored --nocapture
/// ```
#[test]
#[ignore = "diagnostic dump; run with P4RS_CAPTURE=1 and P4D_BIN set"]
fn capture_depot_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        let _ = writeln!(
            std::io::stderr(),
            "skipping capture_depot_records: set P4RS_CAPTURE=1 (and P4D_BIN) to dump records"
        );
        return;
    }
    let Some(server) = TestServer::start("capture") else {
        skip("capture_depot_records");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();

    let depots = c
        .run_records(&mut ui, "depots", Vec::new())
        .expect("depots");
    dump_records("depots", &depots);

    let default_spec = c
        .run_records(
            &mut ui,
            "depot",
            vec!["-o".to_string(), "depot".to_string()],
        )
        .expect("depot -o depot");
    dump_records("depot -o depot", &default_spec);

    // Template for a not-yet-existing stream depot: shows StreamDepth default.
    let new_stream = c
        .run_records(
            &mut ui,
            "depot",
            vec!["-o".to_string(), "newstream".to_string()],
        )
        .expect("depot -o newstream");
    dump_records("depot -o newstream", &new_stream);

    // Probe the save path: without bridge form-input support this fails fast
    // (it must not run `depot -i`, whose default InputData blocks on stdin).
    // Printed, not asserted, so the capture run is informational.
    let template = c.depot_spec("projects").expect("depot_spec template");
    let save = c.save_depot_spec(&template);
    let _ = writeln!(
        std::io::stderr(),
        "\n===== save_depot_spec(projects) probe =====\n  result = {:?}",
        save.map(|_| "ok")
    );
}

/// The default depot lists as a typed record: type `local`, a real creation
/// epoch, and a non-empty map. This exercises the read path end to end.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test depot -- --ignored`"]
fn depots_lists_default_depot_typed() {
    let Some(server) = TestServer::start("list") else {
        skip("depots_lists_default_depot_typed");
        return;
    };

    let mut c = server.connect();
    let depots = c.depots().expect("typed depots against a live p4d");

    let default = depots
        .iter()
        .find(|d| d.name == "depot")
        .expect("fresh server auto-provisions a depot named 'depot'");
    assert_eq!(default.depot_type, DepotType::Local);
    assert!(default.time > 0, "creation epoch should be populated");
    assert!(!default.map.is_empty(), "map should be populated");
}

/// `depot -o` on a not-yet-existing name returns a template spec, typed. This
/// is the read path a caller uses before creating a new depot.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test depot -- --ignored`"]
fn depot_spec_returns_typed_template() {
    let Some(server) = TestServer::start("spec") else {
        skip("depot_spec_returns_typed_template");
        return;
    };

    let mut c = server.connect();
    let spec = c
        .depot_spec("projects")
        .expect("typed depot -o against a live p4d");

    assert_eq!(spec.depot, "projects");
    // A local-depot template renders form text that omits the server-managed
    // Date and carries the required fields.
    let text = spec.to_spec_text();
    assert!(text.contains("Depot:\tprojects\n\n"));
    assert!(text.contains("Type:\t"));
    assert!(!text.contains("Date:"), "form text must omit Date");
}

/// End-to-end round-trip: read a template, edit its description, save it, then
/// re-read and confirm the change persisted -- and that the new depot appears
/// in the listing.
///
/// Saving via `depot -i` needs the C++ bridge to override
/// `ClientUser::InputData`; the current bridge exposes no form-input path
/// (and the SDK default blocks reading stdin -- verified live), so on this
/// build `save_depot_spec` fails fast with an `Err` carrying the rendered
/// form. The test asserts the full round-trip when saving is supported and
/// otherwise verifies the fail-fast contract (an error, promptly, with the
/// form text attached), keeping the read paths -- which do work -- covered.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test depot -- --ignored`"]
fn depot_spec_save_round_trip() {
    let Some(server) = TestServer::start("roundtrip") else {
        skip("depot_spec_save_round_trip");
        return;
    };

    let mut c = server.connect();

    // Read paths always work.
    let before = c.depots().expect("depots before save");
    assert!(
        before.iter().any(|d| d.name == "depot"),
        "default depot should be listed"
    );

    let mut spec = c.depot_spec("projects").expect("template for new depot");
    assert_eq!(spec.depot, "projects");
    let desc = "Round-trip test depot.";
    spec.description = Some(desc.to_string());

    match c.save_depot_spec(&spec) {
        Ok(()) => {
            let depots = c.depots().expect("depots after save");
            assert!(
                depots.iter().any(|d| d.name == "projects"),
                "saved depot should now be listed"
            );

            let reread = c.depot_spec("projects").expect("re-read saved spec");
            assert_eq!(
                reread.description.as_deref().map(str::trim_end),
                Some(desc),
                "description should round-trip through save"
            );
        }
        Err(e) => {
            // Fail-fast contract: the error must carry the rendered form so a
            // caller can submit it out-of-band.
            let p4_rs::errors::Error::SerializationError(_, m) = &e else {
                panic!(
                    "save without bridge input support should fail with the form attached, got {e:?}"
                );
            };
            let form = m.get("spec").expect("error should carry the form text");
            assert!(form.contains("Depot:\tprojects\n\n"));
            assert!(form.contains(&format!("Description:\n\t{desc}\n")));
            let _ = writeln!(
                std::io::stderr(),
                "depot_spec_save_round_trip: save unsupported on this build \
                 (bridge has no ClientUser::InputData); verified fail-fast error \
                 instead of the write assertions."
            );
        }
    }
}
