#pragma once
#include "rust/cxx.h"

#include <memory>
#include "p4/p4api-2021.1.2179737-vs2017_static/include/p4/clientapi.h"
#include "p4/p4api-2021.1.2179737-vs2017_static/include/p4/error.h"

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
    void set_argv(rust::Vec<rust::String> args);

    std::unique_ptr<P4Error> finalizer();
    std::unique_ptr<P4Error> run(P4ClientUser& ui, rust::Str command);

private:
    ClientApi api;
};

std::unique_ptr<P4ClientApi> new_client_api();

struct UICallbackImplementation;

/// P4ClientUser is an implementation of ClientUser in C++ to create a
/// Rust compatible (but not ergonomic) class
class P4ClientUser : public ClientUser {
public:
    P4ClientUser(UICallbackImplementation* callback);

    // C++ Callback functions from p4api::ClientUser
    virtual void Message( Error *err );

    // Rust callback functions used
    // We're using a raw pointer because we never give this ownership
    UICallbackImplementation* impl;
};

std::unique_ptr<P4ClientUser> new_client_user( UICallbackImplementation* callback ) ;

//std::unique_ptr<P4Error> new_error();
