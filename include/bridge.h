#pragma once
#include "rust/cxx.h"

#include <memory>
#include <string>

// The Perforce C++ API. The SDK's own include directory is added to the compiler
// include path by build.rs (from P4API_PATH), so these are version-independent --
// do not hardcode the SDK release here.
#include "p4/clientapi.h"
#include "p4/error.h"

//struct ErrorGeneric;
struct ErrID;

class P4Error {
public:
    P4Error();
    Error error;

    bool is_error() const;
    int severity() const;
    rust::Vec<ErrID> errors() const;

    rust::String get(rust::Str s);
};

std::unique_ptr<P4Error> placeholder_error();

class P4ClientUser;

/// P4ClientApi is an wrapper of the ClientApi in C++ to create a
/// Rust compatible class. This is a low level, non-idiomatic API.
class P4ClientApi {
public:
    P4ClientApi();
    rust::String get_version();
    std::unique_ptr<P4Error> init();

    void set_program(rust::Str program);
    void set_port(rust::Str port);
    void set_user(rust::Str user);
    void set_client(rust::Str client);
    // Path to the tickets file `login` reads/writes. Left unset, the API uses
    // its default (P4TICKETS / ~/.p4tickets); pointing it elsewhere keeps a
    // process (or a test) from touching the user's shared tickets file.
    void set_ticket_file(rust::Str path);
    void set_argv(rust::Vec<rust::String> args);

    std::unique_ptr<P4Error> finalizer();
    std::unique_ptr<P4Error> run(P4ClientUser& ui, rust::Str command);

private:
    ClientApi api;
};

std::unique_ptr<P4ClientApi> new_client_api();

struct UICallbackProxy;

/// P4ClientUser is an implementation of ClientUser in C++ to create a
/// Rust compatible (but not ergonomic) class
class P4ClientUser : public ClientUser {
public:
    P4ClientUser(UICallbackProxy* callback);

    // C++ Callback functions from p4api::ClientUser
    virtual void Message( Error *err );
    virtual void HandleError( Error *err );
    virtual void OutputStat( StrDict *varList );
    virtual void InputData( StrBuf *strbuf, Error *e );
    virtual void Prompt( const StrPtr &msg, StrBuf &rsp, int noEcho, Error *e );

    // Provide the data the next command will read as input (e.g. the spec form
    // for `client -i` / `user -i`). Consumed by the first InputData callback.
    void set_input(rust::Str input);

    // Provide the response to the next server Prompt -- e.g. the password
    // `login` asks for. Consumed by the first Prompt callback. Kept separate
    // from `input` because a single run can both read input and be prompted.
    void set_prompt_response(rust::Str response);

    // Warnings and errors reported during the most recent Run. Info-level
    // messages go to the Rust callback; everything worse accumulates here so
    // P4ClientApi::run can return it instead of dropping it.
    Error errors;

    // Pending input for InputData, delivered at most once per set_input.
    std::string input;
    bool has_input = false;

    // Pending response for Prompt (e.g. a password), delivered at most once
    // per set_prompt_response.
    std::string prompt_response;
    bool has_prompt_response = false;

    // Rust callback functions used
    // We're using a raw pointer because we never give this ownership
    UICallbackProxy* impl;
};

std::unique_ptr<P4ClientUser> new_client_user(UICallbackProxy* callback) ;

