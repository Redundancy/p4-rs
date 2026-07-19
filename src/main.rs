//! Thin binary driver for the `p4_rs` library -- see src/lib.rs for the crate
//! documentation and the client/commands/errors modules.

use p4_rs::client;
use p4_rs::commands;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("p4-rs Creating and connecting");
    use commands::info::Options;

    let mut c = client::Options::new()
        .set_program("foo.rs")
        .set_port("localhost:1666")
        .connect()?;

    let info_opts = Options::new().shortened();
    let r = c.info(&info_opts)?;
    println!("Result: {:?}", r);
    println!("User name: {}", r.user_name);

    Ok(())
}
