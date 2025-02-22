mod errors;
mod client;

use crate::ffi::{new_client_user, P4ClientApi, P4ClientUser};
use cxx::UniquePtr;
use miette::Diagnostic;
use serde_json;
use std::convert::TryFrom;
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
use std::fmt::{Debug, Display, Formatter};
use thiserror::Error;
use log::info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("p4-rs Creating and connecting");

    let c = Options::new()
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
    let mut ui = UserInterface::new();

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

pub struct Options {
    program: Option<String>,
    port: Option<String>,
}

/// Client is the Rust-facing implementation of a P4ClientApi
/// it exists to wrap the bridge implementation, and should look like idiomatic Rust
/// (hiding the bridge types) in a natural Rust interface
pub struct Client {
    internal_client: cxx::UniquePtr<ffi::P4ClientApi>,
}

impl Client {
    pub fn new(c: cxx::UniquePtr<P4ClientApi>) -> Self {
        Self { internal_client: c }
    }
}

/// UserInterface is a user-facing wrapper of P4ClientUser
/// it should be a usable, idiomatic rust type
pub struct UserInterface {
    internal: cxx::UniquePtr<P4ClientUser>,
    callback: Box<UICallbackImplementation>,
}

impl UserInterface {
    ///  We create a UserInterface and immediately create a ClientUser, which is a C++ object.
    ///  The ClientUser needs a rust object on which to call callbacks,
    ///  and this is the purpose of the UICallbackImplementation.
    ///
    /// Safety: the callback object is owned by the UserInterface. The callback is only
    /// called in the context of the P4ClientUser, which is also owned by the user interface.
    pub fn new() -> UserInterface {
        let mut x = Box::new(UICallbackImplementation { value: None });
        let cb: *mut UICallbackImplementation = &mut *x;
        UserInterface {
            internal: unsafe { new_client_user(cb) },
            callback: x,
        }
    }
}

#[derive(Error, Diagnostic, Debug)]
#[diagnostic(help("try doing this instead"))]
pub enum Error {
    #[error("Oops it blew up")]
    RawError(#[from] P4InternalError),
}

/// This is a user-facing error type for low level usage
#[derive(Error, Diagnostic)]
pub struct P4InternalError {
    internal_error: cxx::UniquePtr<ffi::P4Error>,
}

impl P4InternalError {
    fn new(internal_error: cxx::UniquePtr<ffi::P4Error>) -> Self {
        Self { internal_error }
    }
}

/// Severity is based on the internal enumeration
pub enum Severity {
    Info,   // (1) something good happened
    Warn,   // (2) something not good happened
    Failed, // user did somthing wrong
    Fatal,  // system broken -- nothing can continue
}

impl Options {
    pub fn new() -> Options {
        Options {
            port: None,
            program: None,
        }
    }

    pub fn set_program(mut self, program: &str) -> Options {
        self.program = Some(program.to_string());
        self
    }

    pub fn set_port(mut self, p: &str) -> Options {
        self.port = Some(p.to_string());
        self
    }

    pub fn connect(mut self) -> Result<Client, P4InternalError> {
        let mut connection = ffi::new_client_api();
        (&mut self).pre_init_settings(&mut connection);

        let err = connection.as_mut().unwrap().init(); // watch out for drop without finalizer
        let client = Client::new(connection);
        if err.is_error() {
            return Err(P4InternalError::new(err));
        }
        Ok(client)
    }

    /// Settings that must be applied after ClientApi creation
    /// but before connection.
    fn pre_init_settings(&mut self, api: &mut UniquePtr<P4ClientApi>) {
        let mut api = api.as_mut().unwrap();

        let v = match &self.program {
            Some(program) => program.as_str(),
            None => "p4-rs",
        };
        api.as_mut().set_program(v);

        if let Some(port) = &self.port {
            api.as_mut().set_port(port.as_str());
        }
    }
}

impl Client {
    pub fn run(
        &mut self,
        ui: &mut UserInterface,
        command: &str,
        args: Vec<String>,
    ) -> Result<serde_json::Value, Error> {
        println!("Running \"{}\"", command);
        
        let mut api = self.internal_client.as_mut().unwrap();
        api.as_mut().set_argv(args);
        api.as_mut().run(ui.internal.pin_mut(), command);

        Ok(ui.callback.value.take().unwrap().into())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(api) = self.internal_client.as_mut() {
            let e = api.finalizer();
            if e.is_error() {
                println!("{}", P4InternalError::new(e));
            }
        }
    }
}

#[derive(Debug)]
pub struct ErrorID {
    pub sub_code: i32,
    pub subsystem: Subsystem,
    pub generic: i32,
    pub arg_count: i32,
    pub severity: i32,
    pub unique_code: i32,
    pub format_string: String,
}

impl Error {}

fn expand_error_id(e: ffi::ErrID) -> ErrorID {
    let code = e.id;
    ErrorID {
        sub_code: (code >> 0) & 0x3ff,
        subsystem: Subsystem::try_from(code >> 10 & 0x3f).expect("invalid subsystem"),
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
            3 => f.write_fmt(format_args!(
                "P4 Failed... of {:?} errors",
                errors
                    .drain(..)
                    .map(|e| expand_error_id(e))
                    .collect::<Vec<ErrorID>>()
            )),
            4 => f.write_fmt(format_args!("P4 Fatal Error... of {count} errors")),
            _ => f.write_fmt(format_args!("unhandled error severity of {count} errors")),
        }
    }
}

impl Display for P4InternalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Subsystem {
    OS = 0,                   // OS error
    Support = 1,              // Misc support
    Librarian = 2,            // librarian
    RPC = 3,                  // messaging
    Database = 4,             // database
    DataBaseSupport = 5,      // database support
    DataManager = 6,          // data manager
    Server = 7,               // top level of server
    Client = 8,               // top level of client
    Info = 9,                 // pseudo subsystem for information messages
    Help = 10,                // pseudo subsystem for help messages
    Spec = 11,                // pseudo subsystem for spec/comment messages
    FtpServer = 12,           // P4FTP server
    Broker = 13,              // Perforce Broker
    P4VClient = 14,           // P4V and other Qt based clients
    P4X3Server = 15,          // P4X3 server
    GraphDepot = 16,          // graph depot messages
    Script = 17,              // scripting
    ServerOverflow = 18,      // server overflow
    DataManagerOverflow = 19, // dm overflow
}

impl TryFrom<i32> for Subsystem {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Subsystem::OS),
            1 => Ok(Subsystem::Support),
            2 => Ok(Subsystem::Librarian),
            3 => Ok(Subsystem::RPC),
            4 => Ok(Subsystem::Database),
            5 => Ok(Subsystem::DataBaseSupport),
            6 => Ok(Subsystem::DataManager),
            7 => Ok(Subsystem::Server),
            8 => Ok(Subsystem::Client),
            9 => Ok(Subsystem::Info),
            10 => Ok(Subsystem::Help),
            11 => Ok(Subsystem::Spec),
            12 => Ok(Subsystem::FtpServer),
            13 => Ok(Subsystem::Broker),
            14 => Ok(Subsystem::P4VClient),
            15 => Ok(Subsystem::P4X3Server),
            16 => Ok(Subsystem::GraphDepot),
            17 => Ok(Subsystem::Script),
            18 => Ok(Subsystem::ServerOverflow),
            19 => Ok(Subsystem::DataManagerOverflow),
            _ => Err(()),
        }
    }
}

/// UICallbackImplementation is exposed to C++ and handles message callbacks from P4ClientUser
pub struct UICallbackImplementation {
    value: Option<serde_json::map::Map<String, serde_json::Value>>,
}

impl UICallbackImplementation {
    fn message(&mut self, message: &str) {
        let mut value = self
            .value
            .take()
            .unwrap_or_else(|| serde_json::map::Map::new());

        let m = message.to_string();
        if let Some((a, b)) = m.split_once(":") {
            value.insert(a.to_string(), b.trim().to_string().into());
        } else {
            info!("message: {}", message);
        }

        self.value = Some(value);
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
        type UICallbackImplementation;

        fn message(self: &mut UICallbackImplementation, message: &str);

    }

    unsafe extern "C++" {
        // One or more headers with the matching C++ declarations. Our code
        // generators don't read it, but it gets #include'd and used in static
        // assertions to ensure our picture of the FFI boundary is accurate.
        include!("p4/include/bridge.h");

        /// P4ClientApi is the Rust bridge type for the ClientApi class in the P4 SDK
        /// as such, it is very much intended to be a cxx compatible wrapper, not idiomatic
        type P4ClientApi;

        fn new_client_api() -> UniquePtr<P4ClientApi>;
        fn get_version(self: Pin<&mut P4ClientApi>) -> String;
        fn init(self: Pin<&mut P4ClientApi>) -> UniquePtr<P4Error>;
        fn set_program(self: Pin<&mut P4ClientApi>, program: &str);
        fn set_port(self: Pin<&mut P4ClientApi>, port: &str);

        fn finalizer(self: Pin<&mut P4ClientApi>) -> UniquePtr<P4Error>;

        fn set_argv(self: Pin<&mut P4ClientApi>, args: Vec<String>);

        fn run(
            self: Pin<&mut P4ClientApi>,
            ui: Pin<&mut P4ClientUser>,
            command: &str,
        ) -> UniquePtr<P4Error>;

        // setArgv/C
        // run

        type P4Error;
        fn is_error(self: &P4Error) -> bool;
        fn severity(self: &P4Error) -> i32;
        fn errors(self: &P4Error) -> Vec<ErrID>;
        fn get(self: Pin<&mut P4Error>, s: &str) -> String;

        /// P4ClientUser is supposed to be an implementation of the ClientUser class
        type P4ClientUser;
        unsafe fn new_client_user(cb: *mut UICallbackImplementation) -> UniquePtr<P4ClientUser>;

    }
}
