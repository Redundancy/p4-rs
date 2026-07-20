# P4-RS
This is a **personal project** in building a Rust wrapper for the Perforce C++ API - it's built in bursts rather than
sustained effort, and while it may never be "usable" in a production sense, it's come a good deal further than that caveat
once implied (see [Reasonableness of a usable implementation](#reasonableness-of-a-usable-implementation)).

My desire would be to see something that can be easily used in a few ways:
1) As a typesafe layer enabling a very clean rust-native implementation
2) As a non-typesafe layer enabling usage of commands that might not be implemented yet, or that have gaps / errors
3) As an alternative CLI setup that's compatible with the existing CLI, but implementing more useful JSON output
4) As a library that can be wrapped for other languages, hopefully extending a more ergonomic implementation that doesn't need as much effort to wrap for your purposes

Many Perforce client libraries force users to deal with the issue that most responses are effectively a line-based untyped
response. This frequently forces users to build a wrapper around those libraries that parses and extracts the data they want.
The Perforce implementation of JSON output for the current CLI doesn't actually simplify parsing / ingesting this.

## Reasonableness of a usable implementation
When I started, the honest expectation was that a broad, fully-featured surface was more than a
personal project could sustain — it's not my day job, and hand-writing a typed wrapper for command
after command is slow going.

AI-assisted development has changed that calculus. It's quite reasonable now to stand up a large
command surface in a weekend — the breadth you see here — without deep, line-by-line review of
every path, and I think that's a fine place for this project to sit. Each command is still shaped
*capture-first* against a real server and covered by integration tests that spin up an actual
`p4d`, so the common paths are genuinely exercised; what hasn't happened is the hardening of the
long tail of setups and errors that only real usage surfaces.

As an individual, there's still no reasonable way I can see and test every possible setup and
potential error — but that's now a "review and harden as it gets used" note rather than a reason
the project stalls. If anyone actually leans on this, the gaps can be closed over time.

# Current State

Both of the layering goals above now work against a live server.

**`(2)` the non-typesafe layer** — `Client::run_records(&mut ui, "<cmd>", args)` runs any
command in the server's *tagged* protocol and hands back the raw records as
`Vec<HashMap<String, String>>`. That's the escape hatch for anything without a typed wrapper
yet, and `UserInterface::set_input` lets you feed a spec form to `-i` commands.

**`(1)` the typesafe layer** — a growing set of commands return real Rust types built on top
of that. Spec commands round-trip: read with `-o` into a typed struct, mutate it, and save it
back with `-i`.

Implemented so far:

- **Server / admin:** `info`, `users` / `user`, `clients` / `client`, `depots` / `depot`,
  `groups` / `group`, `labels` / `label`, `branches` / `branch`, `counters`
- **Changelists:** `changes`, `change`, `describe`, `submit`
- **Files:** `add`, `edit`, `delete`, `revert`, `sync`, `opened`, `fstat`, `where`, `have`
- **Auth:** `login` / `login -s` / `logout` (typed ticket + expiry)

The typed edit workflow, for example:

```rust
let mut c = client::Options::new()
    .set_port("localhost:1666")
    .set_client("my-ws")
    .connect()?;

c.add(&["/work/hello.txt"])?;                    // -> Vec<FileAction>
let submitted = c.submit("add hello")?;          // -> SubmitResult { change, files }
println!("submitted as change {}", submitted.change);

for f in c.opened(&opened::Options::new())? {    // -> Vec<OpenedFile>, fully typed
    println!("{} open for {}", f.depot_file, f.action);
}
```

Using the builder pattern for options makes a lot of sense to me in terms of being both type
safe, literate, and providing great visibility into what is possible within Rust using code
completion etc.

Two disciplines keep the types honest: every struct is shaped from records **captured against
a real p4d** (the server's tagged output has genuine quirks — lowercase-vs-capitalised keys,
epoch-vs-formatted dates, `//client/...`-vs-local paths — that guessing gets wrong), and every
command has **integration tests that spin up a throwaway `p4d`** and exercise it end to end.

Where a value is a stable set it becomes an `enum` with a strict parse; where the set grows
across server versions it gets an `Other(String)` fallback so a listing never fails on an
unfamiliar value; server-managed fields (Update/Access stamps) are read-only and never sent
back on save.

# Design Notes
## Error Handling

Errors surface on `errors::Error` in three shapes:

* `SerializationError` — the serde-style "I couldn't build the type I expected from the
  response I got". It carries the raw record map alongside the serde error, so when a struct
  fails to deserialize you can see exactly what the server actually sent.
* `RawError` — a `P4InternalError` wrapping an error straight from P4, with its severity,
  subsystem, and individual error ids decoded.
* `SpecError` — spec text that couldn't be parsed or built.

What's *not* here is a typed vocabulary of specific P4 conditions — "file already open", "needs
resolve", and the like — that callers could `match` on instead of inspecting raw ids. That's less
a planned feature than an open-ended one: P4 doesn't hand you a catalogue of the errors a command
can raise, so they only become visible as you actually hit them. The natural way to build it is
opportunistically — promote a raw error into its own variant the first time real usage runs into
it — which is exactly the kind of long-tail hardening that
[accretes with use](#reasonableness-of-a-usable-implementation).

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

Since then the daisy chain has grown two more strands, which are what make the typed commands
possible. The `ClientApi` is put into **tagged** mode, so structured results arrive via
`ClientUser::OutputStat` as key/value dicts rather than pre-formatted text — `CallbackHandler`
gains an `output_stat` for these, and a `RecordsCollector` turns them into the
`Vec<HashMap<String, String>>` that `run_records` returns (and that serde then deserializes into
typed structs). And `ClientUser::InputData` is wired up so a spec form can be fed to `-i`
commands via `UserInterface::set_input` — that's how the `save_*` half of every spec round-trip
works. A later sibling, `ClientUser::Prompt`, answers the server's password prompt from
`UserInterface::set_password` — that's what makes typed `login` possible: the secret goes *in*
much as spec text does, and the issued ticket comes back as an ordinary tagged record.

# Building

This now auto-builds on GitHub Actions (`.github/workflows/build.yml`), on **Windows (MSVC)**
and **Linux (glibc)**, taking cues from [p4python](https://github.com/perforce/p4python): the P4
API and a `p4d` server are downloaded from Perforce filehost at build time (cached, never
committed), OpenSSL/zlib come from Conan, and — the "test-suite by configuring an actual p4
server on the fly" wish — the integration tests spin up that downloaded `p4d` and run the typed
commands against it. `fmt` and `clippy -D warnings` are blocking.

To build locally you need the three dependencies below on disk (the SDK, and OpenSSL + zlib via
Conan). The server-backed tests are `#[ignore]`d unless `P4D_BIN` points at a `p4d`, so a plain
`cargo test` stays green without one.

## Obtaining dependencies
### P4 API
The Perforce API is available from Perforce by visiting their site and agreeing to their license, then finding the version 
that you need on their file server. It cannot be bundled with this project due to the license.

NB: the build reads `P4API_PATH` to locate the SDK (defaulting to a vendored versioned
directory such as `p4api-2025.2.2907753-vs2022_static`), so you can point it wherever the
API is unpacked rather than relying on a fixed path.

### OpenSSL
The P4 API is built against one specific OpenSSL, and linking a different one is an ABI
landmine. That version is embedded as a banner in the SDK's `librpc` lib, so `conanfile.py`
and `build.rs` both read it out and stay in lockstep — nothing about OpenSSL is hardcoded.
Check it yourself with `strings p4api-*/lib/librpc.lib | findstr /B OpenSSL` (Windows).

This uses **Conan 2** to fetch pre-built OpenSSL + zlib (much easier than building OpenSSL
yourself). A small custom deployer reproduces the flat `openssl/` + `zlib/` layout `build.rs`
expects:

```
conan install . --deployer=deploy_flat --deployer-folder=. --build=missing
```

Pinning the Conan profile's `compiler.version` to a config ConanCenter prebuilds (the CI does
this) keeps it a *download*, not a from-source build — so no perl/nasm needed.

### Perforce Server
A working Perforce server is needed for the integration tests. You'll either need an existing
one, or an individual license from Perforce (free for up to 5 users). CI downloads `p4d` from
filehost automatically; locally, point `P4D_BIN` at a `p4d` binary.

## Linux Support
Implemented and exercised by CI (`ubuntu-latest`, the glibc SDK build). macOS is not wired up
yet.



