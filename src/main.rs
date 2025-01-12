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

use std::fmt::{Debug, Display, Formatter, Write};
use std::pin::Pin;
use miette::Diagnostic;
use thiserror::Error;
use crate::ffi::P4ClientApi;

fn main() -> Result<(),Box<dyn std::error::Error>> {
    println!("Creating and connecting");
    let mut c = Options::new().connect();
    if let Err(err) = c {
        let mut err = err;
        println!("{}", err.internal_error.as_mut().unwrap().get("errortext"));
        println!("{}", err.internal_error.as_mut().unwrap().get("host"));
        return Err(Box::new(err));
    }
    let mut c = c.unwrap();
    println!("Running \"info\"");
    c.run("info", Vec::<String>::new())?;
    println!("Done");
    Ok(())
}

pub struct Options ();

pub struct Client {
    internal_client: cxx::UniquePtr<ffi::P4ClientApi>
}

impl Client {
    pub fn new(c: cxx::UniquePtr<P4ClientApi>) -> Self {
        Self{ internal_client: c }
    }
}

#[derive(Error, Diagnostic, Debug)]
#[diagnostic(help("try doing this instead"))]
pub enum Error {
    #[error("Oops it blew up")]
    RawError(#[from] P4InternalError),
}

#[derive(Error, Diagnostic)]
pub struct P4InternalError {
    internal_error: cxx::UniquePtr<ffi::P4Error>
}

impl P4InternalError {
    fn new(internal_error: cxx::UniquePtr<ffi::P4Error>) -> Self {
        Self { internal_error }
    }
}


/// Severity is based on the internal enumeration
pub enum Severity {
    Info,	// (1) something good happened
    Warn,	// (2) something not good happened
    Failed,	// user did somthing wrong
    Fatal,	// system broken -- nothing can continue
}

impl Options {
    pub fn new() -> Options {
        Options()
    }

    pub fn connect(mut self) -> Result<Client,P4InternalError> {
        let mut connection = ffi::new_client_api();
        (&mut self).pre_init_settings();
        let err = connection.as_mut().unwrap().init(); // watch out for drop without finalizer
        let client = Client::new(connection);
        if err.is_error() {
            return Err(P4InternalError::new(err));
        }
        Ok(client)
    }

    /// Settings that must be applied after ClientApi creation
    /// but before connection.
    fn pre_init_settings(&mut self) {

    }

    pub fn set_program_name(_s : &str) {

    }
}

impl Client {
    pub fn run(&mut self, command: &str, _args: Vec<String>) -> Result<(),Error> {
        self.internal_client.as_mut().unwrap().run(command);
        Ok(())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(api)  = self.internal_client.as_mut() {
            let e = api.finalizer();
            if e.is_error() {
                println!("{}",P4InternalError::new(e));
            }
        }
    }
}

#[derive(Debug)]
pub struct ErrorID {
    pub sub_code: i32,
    pub subsystem: i32,
    pub generic: i32,
    pub arg_count: i32,
    pub severity: i32,
    pub unique_code: i32,
    pub format_string: String
}

impl Error {

}

fn expand_error_id(e : ffi::ErrID) -> ErrorID {
    let code = e.id;
    ErrorID{
        sub_code: (code >> 0) & 0x3ff,
        subsystem: (code >> 10) & 0x3f,
        generic: (code >> 16) & 0xff,
        arg_count: (code >> 24) & 0x0f,
        severity: (code >> 28) & 0x0f,
        unique_code: code & 0xffff,
        format_string: e.fmt,
    }
}

impl Debug for P4InternalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut errors: Vec<ffi::ErrID> = self.internal_error.errors();
        let count = errors.len();
        match self.internal_error.severity() {
            1 => f.write_fmt(format_args!("P4 Info... of {count} errors")),
            2 => f.write_fmt(format_args!("P4 Warning... of {count} errors")),
            3 => f.write_fmt(format_args!("P4 Failed... of {:?} errors", errors.drain(..).map(|e| expand_error_id(e)).collect::<Vec<ErrorID>>())),
            4 => f.write_fmt(format_args!("P4 Fatal Error... of {count} errors")),
            _ => f.write_fmt(format_args!("unhandled error severity of {count} errors"))
        }
    }
}

impl Display for P4InternalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

#[cxx::bridge()]
mod ffi {

    // Any shared structs, whose fields will be visible to both languages.
    #[derive(Debug)]
    struct ErrID {
        id: i32,
        fmt: String,
    }

    extern "Rust" {

        
    }

    unsafe extern "C++" {
        // One or more headers with the matching C++ declarations. Our code
        // generators don't read it, but it gets #include'd and used in static
        // assertions to ensure our picture of the FFI boundary is accurate.
        include!("p4/include/bridge.h");

        type P4ClientApi;

        fn new_client_api() -> UniquePtr<P4ClientApi>;
        fn get_version(self: Pin<&mut P4ClientApi>) -> String;
        fn init(self: Pin<&mut P4ClientApi> ) -> UniquePtr<P4Error>;
        fn finalizer(self: Pin<&mut P4ClientApi> ) -> UniquePtr<P4Error>;
        fn run(self: Pin<&mut P4ClientApi>, command: &str) -> UniquePtr<P4Error>;

        // setArgv/C
        // run

        type P4Error;
        fn is_error(self: &P4Error) -> bool;
        fn severity(self: &P4Error) -> i32;
        fn errors(self: &P4Error) -> Vec<ErrID>;
        fn get(self: Pin<&mut P4Error>, s: &str) -> String;

        type P4ClientUser;
        fn new_client_user() -> UniquePtr<P4ClientUser>;

    }
}