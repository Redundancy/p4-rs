//! Integration tests for the typed `depots` / `depot` commands against a real
//! `p4d`. Server-backed tests are `#[ignore]`d; run with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test depot -- --ignored
//! ```

mod common;

use common::{TestServer, dump_records, skip};
use p4_rs::client;
use p4_rs::commands::depot::DepotType;

/// Capture the raw records behind `depots` and `depot -o` so the typed structs
/// can be shaped to the server's actual keys. Gated on `P4RS_CAPTURE`.
#[test]
#[ignore = "diagnostic dump; run with P4RS_CAPTURE=1 and P4D_BIN set"]
fn capture_depot_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        skip("capture_depot_records (set P4RS_CAPTURE=1)");
        return;
    }
    let Some(server) = TestServer::start("dpt-capture") else {
        skip("capture_depot_records");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();

    let r = c.run_records(&mut ui, "depots", Vec::new());
    dump_records("depots", &r);
    let r = c.run_records(
        &mut ui,
        "depot",
        vec!["-o".to_string(), "depot".to_string()],
    );
    dump_records("depot -o depot", &r);
    // Template for a not-yet-existing stream depot: shows StreamDepth default.
    let r = c.run_records(
        &mut ui,
        "depot",
        vec!["-o".to_string(), "newstream".to_string()],
    );
    dump_records("depot -o newstream", &r);
}

/// The default depot lists as a typed record: type `local`, a real creation
/// epoch, and a non-empty map. This exercises the read path end to end.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test depot -- --ignored`"]
fn depots_lists_default_depot_typed() {
    let Some(server) = TestServer::start("dpt-list") else {
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
    let Some(server) = TestServer::start("dpt-spec") else {
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

/// End-to-end round-trip through the library: read a template, edit its
/// description, save it (`depot -i` via the bridge input channel), then re-read
/// and confirm the change persisted -- and that the new depot is listed.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test depot -- --ignored`"]
fn depot_spec_save_round_trip() {
    let Some(server) = TestServer::start("dpt-roundtrip") else {
        skip("depot_spec_save_round_trip");
        return;
    };

    let mut c = server.connect();

    let before = c.depots().expect("depots before save");
    assert!(
        before.iter().any(|d| d.name == "depot"),
        "default depot should be listed"
    );

    let mut spec = c.depot_spec("projects").expect("template for new depot");
    assert_eq!(spec.depot, "projects");
    let desc = "Round-trip test depot.";
    spec.description = Some(desc.to_string());

    c.save_depot_spec(&spec).expect("save new depot spec");

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
