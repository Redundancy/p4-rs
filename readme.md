# P4-RS
This is a **personal project** in building a Rust wrapper for the Perforce C++ API - as such it should not be expected to get
a whole load of effort in any sustained way and may never be "usable" in a production sense.

My desire would be to see something that can be easily used in a few ways:
1) As a typesafe layer enabling a very clean rust-native implementation
2) As a non-typesafe layer enabling usage of commands that might not be implemented yet, or that have gaps / errors
3) As an alternative CLI setup that's compatible with the existing CLI, but implementing more useful JSON output
4) As a library that can be wrapped for other languages, hopefully extending a more ergonomic implementation that doesn't need as much effort to wrap for your purposes

Many Perforce client libraries force users to deal with the issue that most responses are effectively a line-based untyped
response. This frequently forces users to build a wrapper around those libraries that parses and extracts the data they want.
The Perforce implementation of JSON output for the current CLI doesn't actually simplify parsing / ingesting this.

## Reasonableness of a usable implementation
It's going to be a lot to do to get this code to the point where it's a production ready and fully featured implementation.
It's not my day job, it's just something that I've wanted to see for a while, and I figure that demonstrating the idea 
might inspire others. I don't even have the free time to make significant progress at any speed on it if I wanted to.
Getting as far as I have has been a significant amount of learning, which is what I was looking for in a personal project.

As an individual, there's no reasonable way that I can see and test every possible setup and potential error.

# Current State

Of the above, `(2)` has significant progress - you can login and call `run()` with basic command arguments on a simple local server. 
A lot of modes of the responses from P4 are not implemented yet in the UI object (eg. writing files, line based Map responses).
`(1)` has a single (!) vertical slice - `info` is implemented as illustrated below:

```rust
use commands::info::Options;
let mut c = client::Options::new()
  .set_program("foo.rs")
  .set_port("localhost:1666")
  .connect()?;

let info_opts = Options::new().shortened();
let r = c.info(&info_opts)?;
println!("Result: {:?}", r);
println!("User name: {}", r.user_name);
```

Using the builder pattern for options makes a lot of sense to me in terms of being both type safe, literate, and providing
great visibility into what is possible within Rust using code completion etc.

Result:
```
Result: Info { 
    case_handling: Insensitive, 
    client_address: "127.0.0.1", 
    client_host: "DESKTOP-123456", 
    client_name: "DESKTOP-123456", 
    client_root: None, 
    current_dir: "d:\\projects\\p4", 
    server_address: "localhost:1666", 
    server_root: "C:/temp/p4", 
    server_date: "2025/03/05 07:39:25 -0500 Eastern Standard Time", 
    server_version: "P4D/NTX64/2022.2/2369846 (2022/11/14)", 
    server_uptime: "00:00:03", 
    user_name: "Redundancy" 
}
User name: Redundancy
```
Not all the fields are parsed to native Rust types yet (eg.  date, uptime, client IP).  
Errors exist and can be raised from C++, but are not converted into particularly ergonomic error enums (or types).


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

However, what this illustrates is the basics of the plumbing are viable.

# Desired State
## Error Handling

I expect two basic error types, with an extended set of specific ones:

Firstly, some sort of serde-like "I couldn't build the type I was expecting from the response I got" error.  
Second, a "here's an uncategorized error from P4 that I don't know how to interpret"

After that, I'd expect to pull error types out of the second bucket, and into their own enumeration values to make the handling in
Rust more ergonomic and compatible with a switch. 

## Adding Commands
Once the basics of the handling and parsing are done for the various ways that the P4 Server responds to the client, I expect it to be easy to add new commands and update the types/contents/expected responses of existing ones.

## Handling changes to the server responses
At the moment, the type safe implementation of Rust types is handled by serde. 
This has a significant advantage that it has all the machinery for creating types and ensuring that all fields are initialized in an
adequate way, while also potentially providing a very direct route to JSON marshalling (although the current types have rename directives
to handle the P4 attribute names).

Should serde not be able to fill all required fields of a struct, or if there were a type mismatch, it will return an error.

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
    ClientApi api; // <-- original P4 API object
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
*This* type can be written to be fully idiomatic and obscure the cxx-isms.

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

NB: the build reads `P4API_PATH` to locate the SDK (defaulting to a vendored versioned
directory such as `p4api-2025.2.2907753-vs2022_static`), so you can point it wherever the
API is unpacked rather than relying on a fixed path.

### OpenSSL
The version of the p4 OpenSSL dependency is determined (on windows) by: `strings librpc.lib | findstr /B OpenSSL`

This uses Conan to get the OpenSSL dependencies from a pre-built source.  
`conan install . -g deploy` from within the p4 folder should get OpenSSL and zlib.
This is *significantly* easier than building OpenSSL from scratch yourself and needing to try and install all sorts of dependencies.

### Perforce Server
A working Perforce server is needed for testing. You'll either need an existing one, or to use an individual license from perforce
and download one. At the time of writing, Perforce supports free individual p4 server licenses up to 5 users.

## Linux Support
Not implemented. Probably easy-ish.



