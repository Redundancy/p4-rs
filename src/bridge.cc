#include "p4/include/bridge.h"
#include "p4/src/main.rs.h"
#include <memory>
#include <iostream>

// https://www.perforce.com/manuals/p4api/Content/P4API/clientapi.html
P4ClientApi::P4ClientApi() {}

std::unique_ptr<P4ClientApi> new_client_api() {
  return std::make_unique<P4ClientApi>();
}

rust::String P4ClientApi::get_version() {
    auto str = this->api.GetVersion();
    return std::string(str.Value(), str.Length());
}

std::unique_ptr<P4Error> P4ClientApi::init() {
    StrBuf sb;
    sb.Set( "P4RustTest" );
    auto e = std::make_unique<P4Error>();

    this->api.SetPort("localhost:1666");

    this->api.SetProg(&sb);
    this->api.Init(&e->error);
    return e;
}

std::unique_ptr<P4Error> P4ClientApi::finalizer() {
    auto e = std::make_unique<P4Error>();
    this->api.Final(&e->error);
    return e;
}

std::unique_ptr<P4Error> P4ClientApi::run(rust::Str command) {
    auto e = std::make_unique<P4Error>();
    this->api.Run(command.data(), (ClientUser*)&this->user);
    return e;
}

bool P4Error::is_error() const {
    return this->error.IsError();
}

int P4Error::severity() const {
    return this->error.GetSeverity();
}

rust::Vec<ErrID> P4Error::errors() const {
    auto x = rust::Vec<ErrID>::Vec();
    for (int i = 0; i < this->error.GetErrorCount(); i++) {
        auto e = this->error.GetId(i);
        x.push_back(
            ErrID{
                e->code,
                rust::string(e->fmt),
            }
        );
    }
    return x;
}

rust::String P4Error::get(rust::Str s) {
    StrBuf sb;
    sb.Set( s.data(), s.length());
    auto dict = this->error.GetDict();
    auto var = dict->GetVar(sb);
    return std::string(var->Text(), var->Length());
}

P4Error::P4Error() {}

P4ClientUser::P4ClientUser() {}

std::unique_ptr<P4ClientUser> new_client_user() {
    return std::make_unique<P4ClientUser>();
}

// https://www.perforce.com/manuals/p4api/Content/P4API/clientuser.message.html
void P4ClientUser::Message( Error* err ) {
    if (err == 0) {
        return;
    }

    if (err->IsInfo()) {
        StrBuf buf;
        err->Fmt( buf, EF_PLAIN );
        // TODO: Needs to go back to Rust layer
        auto& s = std::string(buf.Text(), buf.Length());
        std::cout << s << "\n";
    }
}