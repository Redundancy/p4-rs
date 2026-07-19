use std::env;
use std::path::{Path, PathBuf};

/// Default vendored SDK directory, used when `P4API_PATH` is not set.
/// CI (and anyone upgrading) overrides this via the env var so the release is
/// pinned in exactly one place instead of being hardcoded across the sources.
const DEFAULT_P4API: &str = "p4api-2025.2.2907753-vs2022_static";

fn main() {
    // The C++ sources include the crate's headers as "p4/..." (e.g.
    // `#include "p4/include/bridge.h"`), so pin the cxx include prefix to `p4`
    // rather than letting it default to the Cargo package name (`p4-rs`).
    cxx_build::CFG.include_prefix = "p4";

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Single source of truth for which P4 C++ API release we build against.
    let p4api = env::var("P4API_PATH").unwrap_or_else(|_| DEFAULT_P4API.to_string());
    let p4api_dir = resolve(&manifest_dir, &p4api);
    let p4api_include = p4api_dir.join("include");
    let p4api_lib = p4api_dir.join("lib");

    // The SDK, OpenSSL and zlib are resolved through directories, never hardcoded
    // versions -- see include/bridge.h, which just does `#include "p4/clientapi.h"`
    // against this include path.
    cxx_build::bridges(vec!["src/client.rs", "src/errors.rs"])
        .include(&p4api_include)
        .include(manifest_dir.join("openssl/include"))
        .include(manifest_dir.join("zlib/include"))
        .static_crt(true)
        .file("src/bridge.cc")
        .flag_if_supported("-std=c++14")
        .compile("p4api-bridge");

    println!("cargo:rustc-link-search=native={}", p4api_lib.display());
    println!(
        "cargo:rustc-link-search=native={}",
        manifest_dir.join("openssl/lib").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        manifest_dir.join("zlib/lib").display()
    );

    // Perforce's Helix build links the P4 API, OpenSSL and zlib statically, plus a
    // set of system libraries that differs per platform. Keep these platform-gated.
    // The combined libp4api archive is self-contained (on Windows it links alone);
    // if a platform's package splits it into client/rpc/supp, add those here.
    // https://www.perforce.com/manuals/p4api/Content/P4API/client.programming.compiling.html
    if target_os == "windows" {
        // On Windows the archive is libp4api.lib (MSVC adds no `lib` prefix).
        println!("cargo:rustc-link-lib=static=libp4api");

        // OpenSSL import-library names differ by series: 1.0.x ships
        // ssleay32/libeay32, while 1.1.x and 3.x ship libssl/libcrypto. Derive them
        // from the OpenSSL version the SDK was built against (the same banner
        // conanfile.py uses to pin the package) so the two can't drift.
        let (ssl_lib, crypto_lib) = match detect_sdk_openssl(&p4api_dir) {
            Some(v) if v.starts_with("1.0.") => {
                println!(
                    "cargo:warning=p4-rs: SDK links legacy OpenSSL {v}; using ssleay32/libeay32"
                );
                ("ssleay32", "libeay32")
            }
            Some(_) => ("libssl", "libcrypto"),
            None => {
                println!(
                    "cargo:warning=p4-rs: could not read the OpenSSL version from {}; \
                     assuming 1.0.x link names (ssleay32/libeay32)",
                    p4api_lib.join("librpc.lib").display()
                );
                ("ssleay32", "libeay32")
            }
        };
        println!("cargo:rustc-link-lib={ssl_lib}");
        println!("cargo:rustc-link-lib={crypto_lib}");
        // OpenSSL 3 is built with zlib support, so libcrypto references zlib
        // (deflate/inflate/...). Conan's zlib package is `zlib.lib` on Windows.
        println!("cargo:rustc-link-lib=zlib");

        println!("cargo:rustc-link-lib=crypt32");
        println!("cargo:rustc-link-lib=Gdi32");
        println!("cargo:rustc-link-lib=User32");
        println!("cargo:rustc-link-lib=Shell32");
        println!("cargo:rustc-link-lib=Ole32");
        println!("cargo:rustc-link-lib=kernel32");
        println!("cargo:rustc-link-lib=ws2_32");
        println!("cargo:rustc-link-lib=advapi32");

        // CRT linkage, and the known wart it papers over:
        //
        // The whole build (see .cargo/config.toml `+crt-static` and `static_crt(true)`
        // above) targets the STATIC CRT (/MT). But the OpenSSL binaries Conan fetches
        // were built with the DYNAMIC CRT (/MD). Those objects reference the dllimport
        // CRT symbols `__imp__chmod` / `__imp__getch` / `__imp__getpid`, which only
        // exist in the dynamic import libs. `oldnames` supplies the POSIX-name aliases
        // and `Msvcrt` the `__imp_` versions; without both, the link fails with LNK2019
        // on exactly those three symbols. This mixes static and dynamic CRTs.
        //
        // TODO: rebuild OpenSSL against the static CRT (Conan `compiler.runtime=MT`,
        // ideally alongside a move to OpenSSL 1.1.1/3.x) and then delete `Msvcrt`.
        println!("cargo:rustc-link-lib=oldnames");
        println!("cargo:rustc-link-lib=Msvcrt");

        // ConanCenter's prebuilt Release OpenSSL references an ossl_static.pdb
        // that isn't shipped in the package, producing a wall of harmless
        // LNK4099 "PDB not found" warnings. Silence just that warning.
        println!("cargo:rustc-link-arg=/ignore:4099");
    } else if target_os == "linux" {
        // On Linux the archive is libp4api.a, so the lib name is `p4api`
        // (rustc prepends `lib`).
        println!("cargo:rustc-link-lib=static=p4api");

        // OpenSSL and zlib are always libssl/libcrypto/libz on Linux, regardless of
        // the OpenSSL series (the ssleay32/libeay32 split is Windows-only). Conan
        // builds them static.
        println!("cargo:rustc-link-lib=static=ssl");
        println!("cargo:rustc-link-lib=static=crypto");
        println!("cargo:rustc-link-lib=static=z");

        // System libraries the P4 API / static OpenSSL depend on. cxx-build already
        // links the C++ runtime because it compiles bridge.cc as C++.
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=rt");
        println!("cargo:rustc-link-lib=m");
    } else {
        println!(
            "cargo:warning=p4-rs supports the windows and linux targets; \
             target_os={target_os} will not link the P4 API correctly yet."
        );
    }

    println!("cargo:rerun-if-env-changed=P4API_PATH");
    println!("cargo:rerun-if-changed=include/bridge.h");
    println!("cargo:rerun-if-changed=src/bridge.cc");
    println!("cargo:rerun-if-changed=src/client.rs");
    println!("cargo:rerun-if-changed=src/errors.rs");
}

/// Resolve a possibly-relative path against the manifest directory.
fn resolve(manifest_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        manifest_dir.join(p)
    }
}

/// Read the OpenSSL version the SDK was built against, from the `OpenSSL x.y.zL`
/// banner embedded in `lib/librpc.lib` (Windows) or `lib/librpc.a` (Linux), e.g.
/// `1.0.2t` or `3.0.8`. Returns `None` if the lib is missing or has no banner.
fn detect_sdk_openssl(p4api_dir: &Path) -> Option<String> {
    let lib = p4api_dir.join("lib");
    let data = std::fs::read(lib.join("librpc.lib"))
        .or_else(|_| std::fs::read(lib.join("librpc.a")))
        .ok()?;
    let needle = b"OpenSSL ";
    // Scan for the banner's first byte, then confirm the full needle. The lib also
    // holds strings like "OpenSSL compile version %s", so take the first hit that is
    // actually followed by a version number.
    let mut from = 0;
    while let Some(rel) = data[from..].iter().position(|&b| b == needle[0]) {
        let start = from + rel;
        if data[start..].starts_with(needle) {
            if let Some(v) = parse_openssl_version(&data[start + needle.len()..]) {
                return Some(v);
            }
        }
        from = start + 1;
    }
    None
}

/// Parse a leading `N.N.N` (optionally with a single trailing lowercase letter,
/// as in OpenSSL 1.0.2t) from the front of `bytes`.
fn parse_openssl_version(bytes: &[u8]) -> Option<String> {
    let mut v = String::new();
    for &b in bytes {
        let c = b as char;
        if c.is_ascii_digit() || c == '.' {
            v.push(c);
        } else if c.is_ascii_lowercase() && v.contains('.') {
            v.push(c); // trailing patch letter, e.g. the 't' in 1.0.2t
            break;
        } else {
            break;
        }
    }
    if v.matches('.').count() >= 2 && v.starts_with(|c: char| c.is_ascii_digit()) {
        Some(v)
    } else {
        None
    }
}
