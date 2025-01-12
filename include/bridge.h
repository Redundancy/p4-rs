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


class P4ClientUser : private ClientUser {
public:
    P4ClientUser();
    void Message( Error *err );
};


class P4ClientApi {
public:
    P4ClientApi();
    rust::String get_version();
    std::unique_ptr<P4Error> init();
    std::unique_ptr<P4Error> finalizer();
    std::unique_ptr<P4Error> run(rust::Str command);

private:
    ClientApi api;
    P4ClientUser user;
};

std::unique_ptr<P4ClientApi> new_client_api();
std::unique_ptr<P4ClientUser> new_client_user() ;

//std::unique_ptr<P4Error> new_error();
