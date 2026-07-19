//! Integration tests for the file read/inspect commands (sync, opened, fstat,
//! where, have) against a real `p4d`. `#[ignore]`d; run with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test file_inspect -- --ignored
//! ```

mod common;

use common::{TestServer, add_and_submit, create_workspace, skip};
use p4_rs::commands::files::{ChangelistId, OpenAction};
use p4_rs::commands::sync::SyncAction;
use p4_rs::commands::{fstat, opened, sync};

/// After submitting a file: fstat/have/where read it back, and force-sync
/// reports it moving in the workspace -- all typed.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test file_inspect -- --ignored`"]
fn inspect_submitted_file() {
    let Some(server) = TestServer::start("inspect") else {
        skip("inspect_submitted_file");
        return;
    };

    let (mut c, work) = create_workspace(&server, "insp-ws");
    let depot = add_and_submit(&mut c, &work, "hello.txt", "hello world\n", "add hello");
    assert_eq!(depot, "//depot/hello.txt");

    // fstat: head metadata for the submitted file.
    let stats = c
        .fstat(&["//depot/..."], &fstat::Options::new())
        .expect("fstat");
    assert_eq!(stats.len(), 1);
    let st = &stats[0];
    assert_eq!(st.depot_file, "//depot/hello.txt");
    assert_eq!(st.head_action, Some(OpenAction::Add));
    assert_eq!(st.head_rev, Some(1));
    assert_eq!(st.have_rev, Some(1));
    assert!(st.is_mapped);

    // have: the workspace holds rev 1.
    let have = c.have(&[]).expect("have");
    assert_eq!(have.len(), 1);
    assert_eq!(have[0].have_rev, 1);
    assert_eq!(have[0].depot_file, "//depot/hello.txt");

    // where: all three path forms map.
    let mapped = c.where_files(&["//depot/hello.txt"]).expect("where");
    assert_eq!(mapped[0].depot_file, "//depot/hello.txt");
    assert!(mapped[0].path.ends_with("hello.txt"));

    // force sync reports the file refreshed at rev 1.
    let synced = c
        .sync_paths(&["//depot/..."], &sync::Options::new().force())
        .expect("sync -f");
    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0].action, SyncAction::Refreshed);
    assert_eq!(synced[0].rev, Some(1));
    assert_eq!(synced[0].file_size, Some(12));
}

/// `opened` lists workspace-open files typed, in the default changelist.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test file_inspect -- --ignored`"]
fn opened_lists_open_files() {
    let Some(server) = TestServer::start("opened") else {
        skip("opened_lists_open_files");
        return;
    };

    let (mut c, work) = create_workspace(&server, "opn-ws");
    add_and_submit(&mut c, &work, "a.txt", "a\n", "add a");

    // Nothing open yet.
    assert!(
        c.opened(&opened::Options::new())
            .expect("opened empty")
            .is_empty(),
        "no files open after submit"
    );

    // Open it for edit.
    c.edit(&["//depot/a.txt"]).expect("edit");
    let open = c.opened(&opened::Options::new()).expect("opened");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].depot_file, "//depot/a.txt");
    assert_eq!(open[0].action, OpenAction::Edit);
    assert_eq!(open[0].change, ChangelistId::Default);
    assert_eq!(open[0].have_rev, Some(1));
}
