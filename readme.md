# P4
This is a **personal project** in building a Rust wrapper for the Perforce C++ API - as such it should not be expected to get
a whole load of effort in any sustained way.

In order to make it as idiomatic and easy to use, the goal is to implement basic bindings allowing access to the C++ API first, \
and then wrap the whole thing with a more idiomatic and type safe Rust layer. Eventually, it would be nice to be able to expose everything 
using a CLI that responds with JSON, or libraries that add bindings for other languages, reducing the implementation of wrappers.
The challenge here is that the P4 API is fundamentally driven by whatever the server happens to respond with, usually a line-by-line response
that needs parsing.

It has been my observation over years of working with Perforce clients that *everyone* ends up writing wrapper implementations.
There is simultaneously no guarantee about the server responses, and everyone has implementations that rely heavily on it.
By taking this challenge on in a low level systems language, we 

My desire is to create something that can be easily used in a few ways:
* As a typesafe layer enabling a very clean rust-native implementation
* As a non-typesafe layer enabling usage of commands that might not be implemented yet, or that have gaps / errors
* As an alternative CLI setup that's compatible with the existing CLI, but implementing more useful JSON output
* As a library that can be wrapped for other languages, hopefully extending a more ergonomic implementation that doesn't need as much effort to wrap for your purposes

# Current State

Goals from above aside, the current implementation has some *very basics* just about working.  
You can connect to a server:
```rust
let c = client::Options::new()
    .set_program("foo.rs")
    .set_port("localhost:1666")
    .connect()?;
```

You can run `info` and get a `serde_json::Value`:
```rust
let mut ui = client::UserInterface::new();
let v = c.run(&mut ui, "info", Vec::<String>::new())?;
println!("Result: {}", v);
```

There is however, some probably not very great C++ that I'd love to go back to, in order to make some const-ness work:
```C++
    char** c_arg = new char*[args.size()];

    for (size_t i = 0; i < args.size(); ++i) {
        auto s = args[i].size() + 1;
        c_arg[i] = new char[s];
        strcpy_s(c_arg[i], s, args[i].c_str());
    }

    // char *const *
    this->api.SetArgv(args.size(), c_arg);

    for (size_t i = 0; i < args.size(); ++i) {
        delete[] c_arg[i];
    }
    delete[] c_arg;
```
erk. There's probably a better way. PRs from people with a more active working knowledge of C++ appreciated.

The goal would be to provide a version that *might* look more like:
```rust
Sync::new()
    .force()
    .metadata_only()
    .with_global_args(...)
    .run(&mut c)?;
```

# Understanding the CXX Wrapping

CXX will automatically help to translate C++ types that are written in a very particular way and declared.
The reality is that most C++ classes, especially those featured in the Perforce API are not written in that way,
and indeed use their own string classes etc.

This necessitates a first layer of wrapping - taking the C++ classes and writing a C++ wrapper around them which can be translated
and exposed to Rust by CXX. 

Here's an example:
```
class P4ClientApi {
public:
    P4ClientApi();
    void set_program(rust::Str program);
    void set_port(rust::Str port);
    void set_argv(rust::Vec<rust::String> args);
private:
    ClientApi api;
};
```
P4ClientApi wholly contains a P4::ClientApi and makes the basic functionality callable from Rust.
This is however not super nice to use, and requires various things like `cxx::UniquePtr`, so we write a pure-rust wrapper 
of the type that we share:
```rust
pub struct Client {
    internal_client: cxx::UniquePtr<ffi::P4ClientApi>,
}
```
*This* type can be written to be fully idiomatic.

### Naming
My general line of thought has been:
* X: original class
* P4X: C++ wrapper
* X: Idiomatic Rust version

## Handling P4 UI Callbacks
If the above was complex, then dealing with the UI callbacks is worse.
At it's core, the P4::ClientApi needs a P4::ClientUser, which it calls as commands are run to provide the output, generally in 
a line-by-line manner using `void Message( Error *err )` (where the `Error` type is more accurately just an event or log).

In order to implement a ClientUser, we have to inherit from it, which we do by implementing a C++ class that does just that.
This is `P4ClientUser`. It's major job is to then know about `UICallbackProxy`, which is a Rust class exposed to C++, and call
the implementation of `message` on that. We use a concrete Rust type, because I'm not sure that we can use a trait.
So now we're handling the callbacks in `UICallbackProxy`, and we dispatch them to a `Box<dyn CallbackHandler>` which is a 
trait that anyone can implement using only Rust - this being the major objective of the whole daisy chain.

# Building

I'd like to get this set up to auto-build using Github actions.  (https://github.com/perforce/p4python may provide inspiration)  
A whole test-suite by configuring an actual p4 server on the fly would be fantastic.

## Obtaining dependencies
### P4 API
The Perforce API is available from Perforce by visiting their site and agreeing to their license, then finding the version 
that you need on their file server. It cannot be bundled with this project due to the license.

### OpenSSL
The version of the p4 OpenSSL dependency is determined (on windows) by: `strings librpc.lib | findstr /B OpenSSL`

This uses Conan to get the OpenSSL dependencies from a pre-built source.  
`conan install . -g deploy` from within the p4 folder should get OpenSSL and zlib.
This is *significantly* easier than building OpenSSL from scratch yourself.

### Perforce Server
A working Perforce server is needed for testing. You'll either need an existing one, or to 

## Linux Support
Not planned yet. Feel free to test and PR, especially if you can add Github actions to fetch dependencies and build.



