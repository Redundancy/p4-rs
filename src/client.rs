use std::any::Any;
use std::collections::HashMap;
use cxx::UniquePtr;
use log::{info, warn};
use serde::Deserialize;
use crate::commands;
use crate::errors::{Error, P4InternalError};

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
    pub fn new(c: cxx::UniquePtr<ffi::P4ClientApi>) -> Self {
        Self { internal_client: c }
    }
}

/// UserInterface is a user-facing wrapper of P4ClientUser
/// it should be a usable, idiomatic rust type
pub struct UserInterface {
    internal: cxx::UniquePtr<ffi::P4ClientUser>,
    callback: Box<UICallbackProxy>,
}

impl UserInterface {
    ///  We create a UserInterface and immediately create a ClientUser, which is a C++ object.
    ///  The ClientUser needs a rust object on which to call callbacks,
    ///  and this is the purpose of the UICallbackProxy.
    ///
    /// Safety: the callback object is owned by the UserInterface. The callback is only
    /// called in the context of the P4ClientUser, which is also owned by the user interface.
    pub fn new() -> UserInterface {
        let mut x = Box::new(UICallbackProxy::new(None));
        let cb: *mut UICallbackProxy = &mut *x;
        UserInterface {
            internal: unsafe { ffi::new_client_user(cb) },
            callback: x,
        }
    }
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
    fn pre_init_settings(&mut self, api: &mut UniquePtr<ffi::P4ClientApi>) {
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

struct JsonValueCollector {
    value: Option<serde_json::Map<String, serde_json::Value>>,
}

impl CallbackHandler for JsonValueCollector {
    fn message(&mut self, message: &str) {
        let mut o = self.value.take().unwrap_or_else(|| serde_json::Map::new());
        if let Some((a, b)) = message.split_once(':') {
            o.insert(a.to_string(), serde_json::Value::String(b.trim_start().to_string()));
        }
        self.value = Some(o);
    }
}

struct MapValueCollector {
    value: HashMap<String, String>
}

impl CallbackHandler for MapValueCollector {
    fn message(&mut self, message: &str) {
        if let Some((a, b)) = message.split_once(':') {
            self.value.insert(a.to_string(), b.trim_start().to_string());
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
        let mut api = self.internal_client.as_mut().unwrap();
        ui.callback.value = Some(Box::new(JsonValueCollector{ value: None }));
        
        api.as_mut().set_argv(args);
        api.as_mut().run(ui.internal.pin_mut(), command);

        // Recover the concrete collector via trait upcasting (dyn CallbackHandler
        // -> dyn Any, stable since Rust 1.86) and a checked downcast.
        let mut m: Box<JsonValueCollector> = (ui.callback.value.take().unwrap() as Box<dyn Any>)
            .downcast()
            .expect("collector should be the JsonValueCollector set above");

        Ok(m.value.take().unwrap_or_default().into())
    }
    
    fn run_map_output(&mut self, ui: &mut UserInterface, command: &str, args: Vec<String>) -> Result<HashMap<String, String>, Error> {
        let mut api = self.internal_client.as_mut().unwrap();
        ui.callback.value = Some(Box::new(MapValueCollector{ value: HashMap::new() }));
        
        api.as_mut().set_argv(args);
        api.as_mut().run(ui.internal.pin_mut(), command);

        // Recover the concrete collector via trait upcasting (dyn CallbackHandler
        // -> dyn Any, stable since Rust 1.86) and a checked downcast.
        let m: Box<MapValueCollector> = (ui.callback.value.take().unwrap() as Box<dyn Any>)
            .downcast()
            .expect("collector should be the MapValueCollector set above");

        // TODO: Check for errors and handle!
        Ok(m.value)
    }
    pub fn info(&mut self, options: &commands::info::Options) -> Result<commands::info::Info, Error> {
        let mut ui = UserInterface::new();
        let m = self.run_map_output(&mut ui, "info", options.get_args())?;
        commands::info::Info::deserialize(
            serde::de::value::MapDeserializer::new(m.clone().into_iter())
        ).map_err(|e| Error::SerializationError(e, m))
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

pub trait CallbackHandler: Any {
    fn message(&mut self, message: &str);
}

/// UICallbackProxy is exposed to C++ and handles message callbacks from P4ClientUser
/// It passes calls to a CallbackHandler, which can be set up to handle different types of message
/// differently, depending on the actual perforce call being made. This avoids trying to teach CXX / C++
/// about rust traits
pub struct UICallbackProxy {
    value: Option<Box<dyn CallbackHandler>>,
}

impl UICallbackProxy {
    fn new(value: Option<Box<dyn CallbackHandler>>) -> UICallbackProxy {
        UICallbackProxy{ value }
    }
    fn message(&mut self, message: &str) {
        if let Some(value) = &mut self.value {
            value.message(message);
        } else {
            warn!("UICallbackProxy called without handler set");
        }
    }
}

#[cxx::bridge()]
pub mod ffi {
    // Any shared structs, whose fields will be visible to both languages.
    #[derive(Debug)]
    struct ErrID {
        id: i32,
        fmt: String,
    }

    extern "Rust" {
        type UICallbackProxy;

        fn message(self: &mut UICallbackProxy, message: &str);

    }

    unsafe extern "C++" {
        // One or more headers with the matching C++ declarations. Our code
        // generators don't read it, but it gets #include'd and used in static
        // assertions to ensure our picture of the FFI boundary is accurate.
        include!("p4/include/bridge.h");
        type P4Error = crate::errors::ffi::P4Error;

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
        
        type P4ClientUser;
        unsafe fn new_client_user(cb: *mut UICallbackProxy) -> UniquePtr<P4ClientUser>;

    }
}
