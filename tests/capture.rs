//! Scratch capture harness: dump real tagged records from a live p4d so typed
//! structs can be shaped against reality. Not part of the test suite proper.

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use p4_rs::client;

struct TestServer {
    child: Child,
    port: String,
    root: PathBuf,
}

impl TestServer {
    fn start(name: &str) -> Option<TestServer> {
        let p4d = std::env::var("P4D_BIN").ok()?;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = format!("localhost:{}", listener.local_addr().unwrap().port());
        drop(listener);
        let root = std::env::temp_dir().join(format!("p4-rs-cap-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&root).expect("create root");
        let child = Command::new(&p4d)
            .arg("-r")
            .arg(&root)
            .arg("-p")
            .arg(&port)
            .spawn()
            .expect("spawn p4d");
        let s = TestServer { child, port, root };
        let addr = s.port.replace("localhost", "127.0.0.1");
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if TcpStream::connect(&addr).is_ok() {
                return Some(s);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("p4d not ready");
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
#[ignore = "capture harness; run manually with P4D_BIN and P4RS_CAPTURE=1 set"]
fn capture_records() {
    // Manual tool, not a test: skip unless explicitly asked for (CI runs the
    // ignored set, and this dumps records rather than asserting anything).
    if std::env::var("P4RS_CAPTURE").is_err() {
        eprintln!("set P4RS_CAPTURE=1 to run the capture harness");
        return;
    }
    let Some(server) = TestServer::start("capture") else {
        eprintln!("P4D_BIN not set");
        return;
    };

    let mut c = client::Options::new()
        .set_program("p4-rs-capture")
        .set_port(&server.port)
        .set_client("cap-ws")
        .connect()
        .expect("connect");

    let mut ui = client::UserInterface::new();

    let dump = |label: &str, r: &Result<Vec<std::collections::HashMap<String, String>>, _>| match r
    {
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
    };

    let r = c.run_records(&mut ui, "user", vec!["-o".into()]);
    dump("user -o", &r);

    // Save own user via input to guarantee a db.user entry exists.
    ui.set_input("User:\tdanrs\n\nEmail:\tdanrs@example.com\n\nFullName:\tDan Test\n");
    let r = c.run_records(&mut ui, "user", vec!["-i".into()]);
    dump("user -i", &r);

    let r = c.run_records(&mut ui, "users", Vec::new());
    dump("users", &r);

    let r = c.run_records(&mut ui, "client", vec!["-o".into(), "cap-ws".into()]);
    dump("client -o (template)", &r);
}
