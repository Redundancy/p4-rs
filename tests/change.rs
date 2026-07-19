//! Integration + capture tests for typed `change` / `describe`.
//!
//! Run the server-backed tests with `P4D_BIN` set:
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test change -- --ignored
//! ```
//! Shape the typed structs against real records with the capture test:
//! ```text
//! P4D_BIN=... P4RS_CAPTURE=1 cargo test --test change -- --ignored --nocapture
//! ```

mod common;

use common::{TestServer, create_client, dump_records, skip};

/// Capture-first harness: dump the real tagged records `change`/`describe`
/// produce, so the typed structs are shaped against reality. Gated on
/// P4RS_CAPTURE so it never runs in a normal `-- --ignored` sweep.
#[test]
#[ignore = "capture harness; run manually with P4D_BIN and P4RS_CAPTURE=1 set"]
fn capture_change_describe_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        eprintln!("set P4RS_CAPTURE=1 to run the capture harness");
        return;
    }
    let Some(server) = TestServer::start("chspec-capture") else {
        eprintln!("P4D_BIN not set");
        return;
    };

    let mut c = server.connect_with_client("chspec-ws");
    create_client(&mut c, "chspec-ws", &server.root.join("ws"));

    let mut ui = p4_rs::client::UserInterface::new();

    // New-change template.
    let r = c.run_records(&mut ui, "change", vec!["-o".into()]);
    dump_records("change -o (new template)", &r);

    let change = common::create_pending_change(&mut c, "capture change");

    let r = c.run_records(&mut ui, "change", vec!["-o".into(), change.clone()]);
    dump_records("change -o <n>", &r);

    let r = c.run_records(&mut ui, "describe", vec!["-s".into(), change.clone()]);
    dump_records("describe -s <n>", &r);
}

/// End-to-end: read the new-change template, set a description, save it, read
/// it back typed, and describe it.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn change_spec_roundtrip_and_describe() {
    let Some(server) = TestServer::start("chspec") else {
        skip("change_spec_roundtrip_and_describe");
        return;
    };

    let mut c = server.connect_with_client("chspec-ws");
    create_client(&mut c, "chspec-ws", &server.root.join("ws"));

    // New-change template.
    let mut spec = c.change_spec(None).expect("change -o new template");
    assert_eq!(spec.change, "new", "template change id is 'new'");
    assert_eq!(spec.status.as_deref(), Some("new"));

    spec.description = "typed change roundtrip".to_string();
    c.save_change_spec(&spec).expect("save change spec");

    // Find the number of the change we just created.
    let mut ui = p4_rs::client::UserInterface::new();
    let latest = c
        .run_records(&mut ui, "changes", vec!["-m".into(), "1".into()])
        .expect("changes -m 1");
    let num = latest
        .first()
        .and_then(|r| r.get("change"))
        .expect("new change number")
        .clone();

    // Re-read it typed: description round-tripped, now pending.
    let reread = c
        .change_spec(Some(&num))
        .expect("change -o <n> for existing change");
    assert_eq!(reread.change, num);
    assert_eq!(reread.status.as_deref(), Some("pending"));
    assert!(
        reread.description.contains("typed change roundtrip"),
        "description should round-trip, got {:?}",
        reread.description
    );

    // Describe it: pending, matching desc, no files (empty pending change).
    let described = c.describe(&num).expect("describe -s <n>");
    assert_eq!(described.change, num.parse::<u64>().unwrap());
    assert_eq!(described.status, "pending");
    assert!(
        described.desc.contains("typed change roundtrip"),
        "describe desc should match, got {:?}",
        described.desc
    );
    assert!(
        described.files.is_empty(),
        "an empty pending change has no described files"
    );
}
