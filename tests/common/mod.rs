//! Shared p4d test harness for integration tests.
//!
//! Each tests/*.rs target compiles this module separately (`mod common;`), and
//! most targets use only part of it -- hence the file-wide dead_code allow.
#![allow(dead_code)]

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use p4_rs::client::{Client, Options, UserInterface};
use p4_rs::commands::info;
use p4_rs::errors::Error;

/// Distinguishes roots when one test process starts several servers.
static ROOT_COUNTER: AtomicU32 = AtomicU32::new(0);

/// A throwaway `p4d` server rooted in a unique temp directory, killed (and its
/// root removed) on drop.
pub struct TestServer {
    child: Child,
    pub port: String,
    pub root: PathBuf,
}

impl TestServer {
    /// Start `p4d` on a free localhost port, or return `None` if `P4D_BIN` is
    /// not set (the signal to skip the test).
    ///
    /// Hardened for parallel test runs: if p4d loses the free-port race to a
    /// sibling process (or the port got snatched so we'd connect to *someone
    /// else's* server), we detect it and retry with a fresh port.
    pub fn start(name: &str) -> Option<TestServer> {
        let p4d = std::env::var("P4D_BIN").ok()?;
        for _attempt in 0..5 {
            match Self::try_start(&p4d, name) {
                Some(server) => return Some(server),
                None => continue,
            }
        }
        panic!("p4d failed to start cleanly after 5 attempts (port races?)");
    }

    fn try_start(p4d: &str, name: &str) -> Option<TestServer> {
        let port_num = free_port();
        let port = format!("localhost:{port_num}");

        let n = ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("p4-rs-it-{}-{}-{}", name, std::process::id(), n));
        std::fs::create_dir_all(&root).expect("create p4d root");

        let child = Command::new(p4d)
            .arg("-r")
            .arg(&root)
            .arg("-p")
            .arg(&port)
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn p4d ({p4d}): {e}"));

        // Own the child immediately: every exit path below (including panics)
        // then runs Drop, which kills, waits, and removes the root -- no
        // zombie processes or stale server roots.
        let mut server = TestServer { child, port, root };

        let addr = format!("127.0.0.1:{port_num}");
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok(Some(_exit)) = server.child.try_wait() {
                // p4d exited early: it lost the bind race. Retry.
                return None;
            }
            if TcpStream::connect(&addr).is_ok() {
                if server.is_ours() {
                    return Some(server);
                }
                // Something answered, but it isn't our p4d (port stolen by a
                // sibling test). Retry with a fresh port.
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "p4d did not start listening on {} within timeout",
            server.port
        );
    }

    /// Positive identity check: the server on our port must report *our* root
    /// directory, so a parallel test can never silently talk to a sibling's
    /// server.
    fn is_ours(&self) -> bool {
        let Ok(mut c) = Options::new()
            .set_program("p4-rs-harness-identity")
            .set_port(&self.port)
            .connect()
        else {
            return false;
        };
        match c.info(&info::Options::new()) {
            Ok(i) => paths_equivalent(&i.server_root, &self.root),
            Err(_) => false,
        }
    }

    pub fn connect(&self) -> Client {
        Options::new()
            .set_program("p4-rs-integration-test")
            .set_port(&self.port)
            .connect()
            .expect("connect to local p4d")
    }

    /// Connect with P4CLIENT set -- required by commands that operate in a
    /// workspace context (e.g. `change -o`).
    pub fn connect_with_client(&self, client_name: &str) -> Client {
        Options::new()
            .set_program("p4-rs-integration-test")
            .set_port(&self.port)
            .set_client(client_name)
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

fn paths_equivalent(a: &str, b: &Path) -> bool {
    let norm = |s: &str| {
        s.replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    };
    norm(a) == norm(&b.to_string_lossy())
}

/// Ask the OS for a free TCP port by binding to :0 and releasing it. Racy by
/// nature; TestServer::start detects lost races and retries.
pub fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

pub fn skip(name: &str) {
    eprintln!("skipping {name}: set P4D_BIN to a p4d executable to run integration tests");
}

/// Pretty-print a run_records result to stderr -- the capture-first workflow
/// for shaping typed structs against real server output. Use from a
/// P4RS_CAPTURE-gated test with `--nocapture`.
pub fn dump_records(label: &str, r: &Result<Vec<HashMap<String, String>>, Error>) {
    match r {
        Ok(records) => {
            eprintln!("=== {label}: {} record(s) ===", records.len());
            for (i, rec) in records.iter().enumerate() {
                let mut keys: Vec<_> = rec.iter().collect();
                keys.sort();
                eprintln!("--- record {i} ---");
                for (k, v) in keys {
                    eprintln!("  {k} = {v:?}");
                }
            }
        }
        Err(e) => eprintln!("=== {label}: ERROR {e:?} ==="),
    }
}

/// Create (or update) a minimal client spec named `name` rooted at `root`, so
/// commands that need an existing client (`change -i`, ...) can run.
pub fn create_client(c: &mut Client, name: &str, root: &Path) {
    let mut spec = c.client_spec(Some(name)).expect("client spec template");
    spec.root = root.to_string_lossy().into_owned();
    spec.description = format!("p4-rs test client {name}");
    c.save_client_spec(&spec).expect("save client spec");
}

/// Create an empty pending changelist and return its number. The connection
/// must have been made with `connect_with_client` for a client that exists
/// (see [`create_client`]) -- `change` runs in workspace context.
pub fn create_pending_change(c: &mut Client, desc: &str) -> String {
    let mut ui = UserInterface::new();

    // Template supplies the correct Client/User for this connection.
    let template = c
        .run_records(&mut ui, "change", vec!["-o".to_string()])
        .expect("change -o template");
    let rec = template.first().expect("change template record");
    let form = format!(
        "Change:\tnew\n\nClient:\t{}\n\nUser:\t{}\n\nStatus:\tnew\n\nDescription:\n\t{}\n",
        rec["Client"], rec["User"], desc
    );

    ui.set_input(&form);
    c.run_records(&mut ui, "change", vec!["-i".to_string()])
        .expect("change -i");

    // The success text ("Change N created.") arrives as an info message, not a
    // record, so read the number back from the newest changelist.
    let latest = c
        .run_records(&mut ui, "changes", vec!["-m".to_string(), "1".to_string()])
        .expect("changes -m 1");
    latest
        .first()
        .and_then(|r| r.get("change"))
        .expect("new change number")
        .clone()
}

/// Create a client rooted at a real on-disk directory and return the connected
/// client plus that workspace root -- the starting point for any file-operation
/// test (add/edit/submit/sync...).
pub fn create_workspace(server: &TestServer, client_name: &str) -> (Client, PathBuf) {
    let work = server.root.join(format!("ws-{client_name}"));
    std::fs::create_dir_all(&work).expect("create workspace dir");
    let mut c = server.connect_with_client(client_name);
    create_client(&mut c, client_name, &work);
    (c, work)
}

/// Write `content` to `rel` within the workspace, `p4 add` it, and submit it,
/// so later tests have a file in the depot to sync/edit/describe. Returns the
/// file's depot path (`//depot/<rel>`).
pub fn add_and_submit(c: &mut Client, work: &Path, rel: &str, content: &str, desc: &str) -> String {
    let path = work.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&path, content).expect("write workspace file");

    let local = path.to_string_lossy();
    c.add(&[local.as_ref()]).expect("add file");
    c.submit(desc).expect("submit file");

    format!("//depot/{}", rel.replace('\\', "/"))
}
