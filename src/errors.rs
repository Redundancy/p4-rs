use std::convert::TryFrom;
use std::fmt::{Debug, Display, Formatter};
use miette::Diagnostic;
use thiserror::Error;


#[derive(Error, Diagnostic, Debug)]
#[diagnostic(help("try doing this instead"))]
pub enum Error {
    #[error("Oops it blew up")]
    RawError(#[from] P4InternalError),
}

/// This is a user-facing error type for low level usage
#[derive(Error, Diagnostic)]
pub struct P4InternalError {
    pub(crate) internal_error: cxx::UniquePtr<ffi::P4Error>,
}

impl P4InternalError {
    pub(crate) fn new(internal_error: cxx::UniquePtr<ffi::P4Error>) -> Self {
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

#[cxx::bridge]
pub mod ffi {
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


        pub type P4Error;
        fn is_error(self: &P4Error) -> bool;
        fn severity(self: &P4Error) -> i32;
        fn errors(self: &P4Error) -> Vec<ErrID>;
        fn get(self: Pin<&mut P4Error>, s: &str) -> String;
        
        fn placeholder_error() -> UniquePtr<P4Error>;
    }
}
