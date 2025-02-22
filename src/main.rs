mod errors;
mod client;

/// Perforce Rust API Wrapper
///
/// There are a number of choices in this wrapper that may be made for the sake of expedience and
/// getting something to work, that might want to be revisted later with more experience of CXX/Rust
/// and usage of the wrapper.
///
/// Other than getting to a point where there's a workable Rust P4 API, one of the main points of this
/// exercise is to try and make a more usable Perforce API surface that is actually typed. The Perforce
/// API underlying this is just whatever the Server feels like returning, line by line,
///  which typically pushes the problem of what to expect and how to expect it down onto consumers.
/// The result of that is that almost every time I've seen someone integrate with P4, they have the own
/// wrapper library to make it a less painful experience.
///
/// While the underlying API (below) won't make any particular assumptions about returns, the intent is to
/// make an opinionated wrapper, and then on top of that an opinionated CLI for a subset of commands that
/// will output "proper" JSON.
///
/// This potentially provides two new options for a lot of consumers - integrate with a CLI that does a lot of
/// heavy lifting for you, or create a wrapper on top of the wrapper (eg. PyO3 for Rust)

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("p4-rs Creating and connecting");

    let c = client::Options::new()
        .set_program("foo.rs")
        .set_port("localhost:1666")
        .connect();

    // TODO: Make standard error from p4 ergonomic
    // TODO: P4InternalError should just be called Error
    if let Err(err) = c {
        let mut err = err;
        if let Some(mut ie) = err.internal_error.as_mut() {
            println!("operation: {}", ie.as_mut().get("operation"));
            println!("host: {}", ie.as_mut().get("host"));
        } else {
            println!("connect error");
        }
        return Err(Box::new(err));
    }
    let mut c = c.unwrap();
    let mut ui = client::UserInterface::new();

    // TODO: keep "run" for the generic case, but expose c.info() -> Result<Info, P4InternalError>
    let v = c.run(&mut ui, "info", Vec::<String>::new())?;
    println!("Result: {}", v);
    
    // TODO: aim for a 
    // let info = Info::new().shortened().run(&mut c)?;
    // let sync = Sync::new()
    //                  .force()
    //                  .metadata_only()
    //                  .with_global_args(...)
    //                  .run(&mut c)?;

    let v = c.run(&mut ui, "clients", vec![])?;
    println!("Result: {}", v);
    Ok(())
}
