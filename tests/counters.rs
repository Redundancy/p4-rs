//! Integration tests for the typed counter commands, exercised against a real
//! `p4d`. Server-backed tests are `#[ignore]`d; run with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test counters -- --ignored
//! ```

mod common;

use common::{TestServer, dump_records, skip};
use p4_rs::client;

/// Capture the raw tagged records the server emits, to shape the typed wrapper
/// against reality. Gated on `P4RS_CAPTURE` so it does not run in the normal
/// `-- --ignored` sweep.
#[test]
#[ignore = "capture-only: set P4RS_CAPTURE=1 and P4D_BIN to dump raw records"]
fn capture_counter_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        skip("capture_counter_records");
        return;
    }
    let Some(server) = TestServer::start("ctr-capture") else {
        skip("capture_counter_records");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();

    let r = c.run_records(
        &mut ui,
        "counter",
        vec!["p4rs-cap".to_string(), "42".to_string()],
    );
    dump_records("counter p4rs-cap 42 (set)", &r);
    let r = c.run_records(&mut ui, "counters", Vec::new());
    dump_records("counters", &r);
    let r = c.run_records(&mut ui, "counter", vec!["p4rs-cap".to_string()]);
    dump_records("counter p4rs-cap (get)", &r);
    let r = c.run_records(&mut ui, "counter", vec!["nonexistent-counter".to_string()]);
    dump_records("counter nonexistent-counter (get)", &r);
}

/// Full lifecycle: set -> get -> list (typed) -> update -> delete -> absent.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test counters -- --ignored`"]
fn counter_roundtrip() {
    let Some(server) = TestServer::start("ctr-roundtrip") else {
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
