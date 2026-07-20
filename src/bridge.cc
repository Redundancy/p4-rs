#include "p4/include/bridge.h"
#include "p4/src/client.rs.h"
#include <memory>
#include <iostream>
#include <string>
#include <vector>


std::unique_ptr<P4Error> placeholder_error() {
    return std::make_unique<P4Error>();
}

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
    // Tagged protocol: structured records arrive via ClientUser::OutputStat as
    // key/value dicts instead of pre-formatted text lines. Must be set before
    // Init. Commands without tagged support still report through Message.
    this->api.SetProtocol("tag", "");
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

void P4ClientApi::set_user(rust::Str user) {
    std::string u(user);
    this->api.SetUser(u.c_str());
}

void P4ClientApi::set_client(rust::Str client) {
    std::string c(client);
    this->api.SetClient(c.c_str());
}

void P4ClientApi::set_ticket_file(rust::Str path) {
    std::string p(path);
    this->api.SetTicketFile(p.c_str());
}

std::unique_ptr<P4Error> P4ClientApi::finalizer() {
    auto e = std::make_unique<P4Error>();
    this->api.Final(&e->error);
    return e;
}

void P4ClientApi::set_argv(rust::Vec<rust::String> args) {
    // Own the argument bytes in std::strings: RAII frees them automatically (no
    // manual new/delete to leak, and exception-safe), std::string keeps the exact
    // bytes, and c_str() gives the NUL-terminated pointers SetArgv expects.
    std::vector<std::string> owned;
    owned.reserve(args.size());
    for (const auto& arg : args) {
        owned.emplace_back(arg.data(), arg.size());
    }

    std::vector<char*> argv;
    argv.reserve(owned.size());
    for (auto& s : owned) {
        argv.push_back(const_cast<char*>(s.c_str()));
    }

    // SetArgv takes char *const * and does not modify the strings.
    this->api.SetArgv(static_cast<int>(argv.size()), argv.data());
}

std::unique_ptr<P4Error> P4ClientApi::run(P4ClientUser& ui, rust::Str command) {
    ui.errors.Clear();
    std::string command_str(command);
    this->api.Run(command_str.c_str(), (ClientUser*)&ui);

    // Surface whatever the ClientUser callbacks accumulated during the run.
    auto e = std::make_unique<P4Error>();
    e->error = ui.errors;
    return e;
}

bool P4Error::is_error() const {
    return this->error.IsError();
}

int P4Error::severity() const {
    return this->error.GetSeverity();
}

rust::Vec<ErrID> P4Error::errors() const {
    auto x = rust::Vec<ErrID>();
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

P4ClientUser::P4ClientUser(UICallbackProxy* cb) {
    this->impl = cb;
}

std::unique_ptr<P4ClientUser> new_client_user(UICallbackProxy* callback) {
    return std::make_unique<P4ClientUser>(callback);
}

// https://www.perforce.com/manuals/p4api/Content/P4API/clientuser.message.html
void P4ClientUser::Message( Error* err ) {
    if (err == 0) {
        return;
    }

    // Warnings and errors are results, not output: accumulate them for run()
    // rather than feeding their text into the output collector.
    if (!err->IsInfo()) {
        this->errors.Merge(*err);
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

void P4ClientUser::HandleError( Error* err ) {
    if (err == 0) {
        return;
    }
    this->errors.Merge(*err);
}

void P4ClientUser::set_input(rust::Str input) {
    this->input.assign(input.data(), input.size());
    this->has_input = true;
}

void P4ClientUser::set_prompt_response(rust::Str response) {
    this->prompt_response.assign(response.data(), response.size());
    this->has_prompt_response = true;
}

// Called when the server asks the client a question (`noEcho` set for secret
// answers like the password `login` requests). We answer with whatever
// set_prompt_response stored, once; with nothing pending we leave the response
// empty, which the server reports as a bad-password/authentication error rather
// than blocking on an interactive read.
void P4ClientUser::Prompt( const StrPtr& msg, StrBuf& rsp, int noEcho, Error* e ) {
    (void)msg;
    (void)noEcho;
    (void)e;
    if (!this->has_prompt_response) {
        rsp.Clear();
        return;
    }
    rsp.Set(this->prompt_response.c_str(), static_cast<int>(this->prompt_response.size()));
    this->has_prompt_response = false;
}

// Called by the server when a command reads input (e.g. `client -i` reads the
// spec form). Delivers whatever set_input stored, once; with nothing pending we
// leave the buffer empty, which the server reports as an empty-input error.
void P4ClientUser::InputData( StrBuf* strbuf, Error* e ) {
    (void)e;
    if (strbuf == nullptr || !this->has_input) {
        return;
    }
    strbuf->Set(this->input.c_str(), static_cast<int>(this->input.size()));
    this->has_input = false;
}

// Tagged-protocol output: one call per record, as a StrDict of key/values.
// Forward each record to the Rust proxy as a vector of pairs.
void P4ClientUser::OutputStat( StrDict* varList ) {
    if (varList == nullptr || this->impl == nullptr) {
        return;
    }

    rust::Vec<KV> vars;
    StrRef var, val;
    for (int i = 0; varList->GetVar(i, var, val); i++) {
        // Protocol bookkeeping, not user data.
        if (var == "func") {
            continue;
        }
        KV kv;
        kv.key = rust::String(var.Text(), var.Length());
        kv.value = rust::String(val.Text(), val.Length());
        vars.push_back(std::move(kv));
    }
    this->impl->output_stat(std::move(vars));
}