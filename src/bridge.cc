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
    auto e = std::make_unique<P4Error>();
    this->api.Init(&e->error);
    return e;
}

void P4ClientApi::set_program(rust::Str program) {
    std::string prog(program);
    this->api.SetProg(prog.c_str());
}

void P4ClientApi::set_port(rust::Str port) {
    std::string p(port);
    this->api.SetPort(p.c_str());
}

std::unique_ptr<P4Error> P4ClientApi::finalizer() {
    auto e = std::make_unique<P4Error>();
    this->api.Final(&e->error);
    return e;
}

void P4ClientApi::set_argv(rust::Vec<rust::String> args) {
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
}

std::unique_ptr<P4Error> P4ClientApi::run(P4ClientUser& ui, rust::Str command) {
    auto e = std::make_unique<P4Error>();
    std::string command_str(command);
    this->api.Run(command_str.c_str(), (ClientUser*)&ui);
    // TODO: How to get an error from the command?
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
    if (dict == 0) {
        return std::string();
    }

    auto var = dict->GetVar(sb);
    if (var == 0) {
        return std::string();
    }

    return std::string(var->Text(), var->Length());
}

P4Error::P4Error() {}

P4ClientUser::P4ClientUser(UICallbackImplementation* cb) {
    this->impl = cb;
}

std::unique_ptr<P4ClientUser> new_client_user(UICallbackImplementation* callback) {
    return std::make_unique<P4ClientUser>(callback);
}

// https://www.perforce.com/manuals/p4api/Content/P4API/clientuser.message.html
void P4ClientUser::Message( Error* err ) {
    if (err == 0) {
        return;
    }
    if (this->impl == nullptr) {
        return;
    }

    StrBuf buf;
    err->Fmt( buf, EF_PLAIN );
    auto s = std::string(buf.Text(), buf.Length());
    this->impl->message(rust::Str(s));
}