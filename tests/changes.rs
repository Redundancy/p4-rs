//! Integration tests for the typed `p4 changes` command.
//!
//! Server-backed tests are `#[ignore]`d; run them with a real p4d:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test changes -- --ignored
//! ```

mod common;

use common::{TestServer, create_client, create_pending_change, dump_records, skip};
use p4_rs::client;
use p4_rs::commands::changes::{ChangeStatus, Options};

/// Capture-first harness: dump the real tagged `changes` records (with and
/// without `-l`) so the typed structs can be shaped against reality. Gated on
/// P4RS_CAPTURE so a normal `cargo test -- --ignored` run doesn't print noise.
///
/// ```text
/// P4RS_CAPTURE=1 cargo test --test changes -- --ignored --nocapture
/// ```
#[test]
#[ignore = "capture harness; run manually with P4D_BIN and P4RS_CAPTURE=1 set"]
fn capture_changes_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        eprintln!("set P4RS_CAPTURE=1 to run the capture harness");
        return;
    }
    let Some(server) = TestServer::start("changes-capture") else {
        eprintln!("P4D_BIN not set");
        return;
    };

    let mut c = server.connect_with_client("chg-cap-ws");
    create_client(&mut c, "chg-cap-ws", &server.root.join("ws"));
    let _ = create_pending_change(&mut c, "capture change one");

    let mut ui = client::UserInterface::new();

    let r = c.run_records(&mut ui, "changes", Vec::new());
    dump_records("changes (no args)", &r);

    let r = c.run_records(&mut ui, "changes", vec!["-l".to_string()]);
    dump_records("changes -l (long desc)", &r);

    let r = c.run_records(
        &mut ui,
        "changes",
        vec!["-s".to_string(), "pending".to_string()],
    );
    dump_records("changes -s pending", &r);
}

/// `changes()` lists a pending changelist we just created, typed.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn changes_lists_pending_change() {
    let Some(server) = TestServer::start("changes") else {
        skip("changes_lists_pending_change");
        return;
    };

    let mut c = server.connect_with_client("chg-ws");
    create_client(&mut c, "chg-ws", &server.root.join("ws"));
    let n = create_pending_change(&mut c, "test change one");

    let listed = c.changes(&Options::new()).expect("list changes");
    let mine = listed
        .iter()
        .find(|ch| ch.change.to_string() == n)
        .expect("created pending change should be listed");

    assert_eq!(mine.status, ChangeStatus::Pending);
    assert!(!mine.user.is_empty(), "user should be populated");
    assert!(mine.change > 0, "change number should be positive");
}
