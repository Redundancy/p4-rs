//! Integration tests for the typed `branches` list and `branch` spec commands.
//!
//! The server-backed tests are `#[ignore]`d; run them with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test branch -- --ignored
//! ```

mod common;

use common::{TestServer, dump_records, skip};

/// Capture-first scratch dump: shape the typed structs against real records.
/// Run manually:
///
/// ```text
/// P4RS_CAPTURE=1 P4D_BIN=... cargo test --test branch -- --ignored --nocapture
/// ```
#[test]
#[ignore = "capture harness; run manually with P4D_BIN and P4RS_CAPTURE=1 set"]
fn capture_branch_records() {
    if std::env::var("P4RS_CAPTURE").is_err() {
        eprintln!("set P4RS_CAPTURE=1 to run the capture harness");
        return;
    }
    let Some(server) = TestServer::start("branch-capture") else {
        eprintln!("P4D_BIN not set");
        return;
    };

    let mut c = server.connect();
    let mut ui = p4_rs::client::UserInterface::new();

    // Template for a branch that doesn't exist yet.
    let r = c.run_records(&mut ui, "branch", vec!["-o".into(), "mybr".into()]);
    dump_records("branch -o mybr (template)", &r);

    // Save one, then dump the list and the re-read spec.
    ui.set_input(
        "Branch:\tmybr\n\nOwner:\tcapture\n\nDescription:\n\tCapture branch.\n\nOptions:\tunlocked\n\nView:\n\t//depot/main/... //depot/rel/...\n",
    );
    let r = c.run_records(&mut ui, "branch", vec!["-i".into()]);
    dump_records("branch -i", &r);

    let r = c.run_records(&mut ui, "branches", Vec::new());
    dump_records("branches", &r);

    let r = c.run_records(&mut ui, "branch", vec!["-o".into(), "mybr".into()]);
    dump_records("branch -o mybr (saved)", &r);
}

/// The create -> modify -> save -> re-read cycle for branch specs, all typed,
/// plus the typed list.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test -- --ignored`"]
fn branch_create_modify_reread_and_list() {
    use p4_rs::commands::client::ViewMapping;

    let Some(server) = TestServer::start("branch") else {
        skip("branch_create_modify_reread_and_list");
        return;
    };

    let mut c = server.connect();

    // A spec for a branch that doesn't exist yet is a defaulted template.
    let mut spec = c.branch_spec("main-to-rel").expect("read branch template");
    assert_eq!(spec.branch, "main-to-rel");
    assert!(
        spec.update.is_none(),
        "unsaved template has no Update stamp"
    );

    // Modify: description and a two-sided view mapping (replacing the
    // template's default `//depot/... //depot/...`).
    spec.description = "Integration test branch.".to_string();
    spec.view = vec![ViewMapping::new("//depot/main/...", "//depot/rel/...")];

    c.save_branch_spec(&spec)
        .expect("save modified branch spec");

    // branches() lists it, typed, with an epoch Update stamp.
    let listed = c.branches().expect("list branches");
    let mine = listed
        .iter()
        .find(|b| b.branch == "main-to-rel")
        .expect("created branch should be listed");
    assert!(
        mine.update > 0,
        "tagged Update should be an epoch timestamp"
    );

    // Re-read: our modifications persisted and the server stamped it.
    let saved = c.branch_spec("main-to-rel").expect("re-read saved spec");
    assert_eq!(saved.description.trim_end(), "Integration test branch.");
    assert!(saved.update.is_some(), "saved spec has an Update stamp");
    assert_eq!(saved.view.len(), 1);
    assert_eq!(saved.view[0].depot, "//depot/main/...");
    assert_eq!(saved.view[0].client, "//depot/rel/...");
}
