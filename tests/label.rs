//! Integration tests for the typed `labels` / `label` wrappers against a real
//! `p4d`. Server-backed tests are `#[ignore]`d; run with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test label -- --ignored
//! ```

mod common;

use common::{TestServer, dump_records, skip};
use p4_rs::client;

/// `labels` on a fresh server returns an empty list -- the command succeeds
/// with zero records rather than erroring.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn labels_empty_on_fresh_server() {
    let Some(server) = TestServer::start("lbl-empty") else {
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
    let Some(server) = TestServer::start("lbl-template") else {
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

/// Full round-trip through the library itself: template -> modify -> save
/// (`label -i` via the bridge input channel) -> list -> re-read.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn label_create_modify_reread() {
    let Some(server) = TestServer::start("lbl-roundtrip") else {
        skip("label_create_modify_reread");
        return;
    };

    let mut c = server.connect();

    let mut spec = c.label_spec("rel-1.0").expect("label -o template");
    spec.description = Some("Release 1.0 label".to_string());
    spec.options.locked = true;
    spec.view = vec!["//depot/rel/...".to_string()];
    c.save_label_spec(&spec).expect("save label spec");

    // labels() lists the newly-created label with an epoch Update > 0.
    let labels = c.labels().expect("labels after save");
    assert_eq!(labels.len(), 1, "one label should be listed");
    let summary = &labels[0];
    assert_eq!(summary.name, "rel-1.0");
    assert!(summary.update > 0, "list Update is epoch seconds > 0");
    assert!(summary.options.locked, "label was created locked");

    // label_spec() re-reads the saved form with everything round-tripped.
    let saved = c.label_spec("rel-1.0").expect("label -o after save");
    assert_eq!(saved.label, "rel-1.0");
    assert_eq!(
        saved.description.as_deref().map(str::trim_end),
        Some("Release 1.0 label")
    );
    assert_eq!(saved.view, vec!["//depot/rel/...".to_string()]);
    assert!(saved.options.locked, "saved label is locked");
    assert!(
        saved.update.is_some(),
        "a saved label has a server-managed Update timestamp"
    );
}

/// Capture harness: dump the raw records behind `labels` and `label -o` so the
/// typed structs can be shaped against reality. Gated on `P4RS_CAPTURE`.
#[test]
#[ignore = "capture-only; run with P4RS_CAPTURE=1 cargo test --test label -- --ignored --nocapture"]
fn capture_label_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        return;
    }
    let Some(server) = TestServer::start("lbl-capture") else {
        skip("capture_label_records");
        return;
    };

    let mut c = server.connect();
    let mut ui = client::UserInterface::new();

    let r = c.run_records(&mut ui, "label", vec!["-o".into(), "rel-1.0".into()]);
    dump_records("label -o (template)", &r);

    let mut spec = c.label_spec("rel-1.0").expect("template");
    spec.description = Some("Release 1.0 label".to_string());
    spec.options.locked = true;
    c.save_label_spec(&spec).expect("save");

    let r = c.run_records(&mut ui, "labels", vec![]);
    dump_records("labels", &r);
    let r = c.run_records(&mut ui, "label", vec!["-o".into(), "rel-1.0".into()]);
    dump_records("label -o (saved)", &r);
}
