//! Integration tests for the file write path (add / edit / delete / revert /
//! submit) against a real `p4d`. `#[ignore]`d; run with:
//!
//! ```text
//! P4D_BIN=/path/to/p4d cargo test --test files -- --ignored
//! ```

mod common;

use common::{TestServer, add_and_submit, create_workspace, skip};
use p4_rs::commands::files::OpenAction;

/// The core edit workflow: add -> submit -> edit -> revert, verified typed at
/// each step against a live server.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test files -- --ignored`"]
fn add_submit_edit_revert() {
    let Some(server) = TestServer::start("files") else {
        skip("add_submit_edit_revert");
        return;
    };

    let (mut c, work) = create_workspace(&server, "files-ws");

    // add
    let file = work.join("hello.txt");
    std::fs::write(&file, "hello\n").unwrap();
    let local = file.to_string_lossy().into_owned();
    let added = c.add(&[&local]).expect("add");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].depot_file, "//depot/hello.txt");
    assert_eq!(added[0].action, OpenAction::Add);
    assert_eq!(added[0].work_rev, Some(1));

    // submit
    let result = c.submit("add hello").expect("submit");
    assert_eq!(result.change, 1, "first submit lands as change 1");
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].depot_file, "//depot/hello.txt");

    // edit
    let edited = c.edit(&[&local]).expect("edit");
    assert_eq!(edited[0].action, OpenAction::Edit);

    // revert restores it, reporting what it was open for
    let reverted = c.revert(&[&local]).expect("revert");
    assert_eq!(reverted.len(), 1);
    assert_eq!(reverted[0].depot_file, "//depot/hello.txt");
    assert_eq!(reverted[0].old_action, Some(OpenAction::Edit));
    assert_eq!(reverted[0].have_rev, Some(1));
}

/// `delete` opens a submitted file for delete; submitting removes it.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test files -- --ignored`"]
fn delete_and_submit_removes_file() {
    let Some(server) = TestServer::start("files-del") else {
        skip("delete_and_submit_removes_file");
        return;
    };

    let (mut c, work) = create_workspace(&server, "del-ws");
    add_and_submit(&mut c, &work, "doomed.txt", "bye\n", "add doomed");

    let deleted = c.delete(&["//depot/doomed.txt"]).expect("delete");
    assert_eq!(deleted[0].action, OpenAction::Delete);

    let result = c.submit("delete doomed").expect("submit delete");
    assert_eq!(result.files[0].action, Some(OpenAction::Delete));
    assert_eq!(result.change, 2, "second submit lands as change 2");
}

/// Reverting a freshly-added (never-submitted) file leaves no have-revision.
#[test]
#[ignore = "requires P4D_BIN; run with `cargo test --test files -- --ignored`"]
fn revert_of_add_has_no_have_rev() {
    let Some(server) = TestServer::start("files-revadd") else {
        skip("revert_of_add_has_no_have_rev");
        return;
    };

    let (mut c, work) = create_workspace(&server, "revadd-ws");
    let file = work.join("new.txt");
    std::fs::write(&file, "new\n").unwrap();
    let local = file.to_string_lossy().into_owned();
    c.add(&[&local]).expect("add");

    let reverted = c.revert(&[&local]).expect("revert");
    assert_eq!(reverted[0].old_action, Some(OpenAction::Add));
    assert_eq!(
        reverted[0].have_rev, None,
        "a reverted add was never synced, so no have-rev"
    );
}
