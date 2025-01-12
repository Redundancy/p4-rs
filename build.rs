use std::{env, io};
use std::fs::File;
use std::io::{Error, Write};
use std::path::{Path};

// OpenSSL 1.0.2t
fn main() {
    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR env variable not set").to_owned();
    let _target_dir = Path::new(out_dir.as_os_str()).ancestors().nth(4).unwrap();

    #[cfg(feature = "disable-dropbox")]
    set_directory_as_dropbox_ignored(_target_dir).expect("should work!");

    println!("cargo:rustc-link-lib=Msvcrt");

    // https://www.perforce.com/manuals/p4api/Content/P4API/client.programming.compiling.html#Compiling_and_linking_Helix_server_applications
    // TODO: Download the latest compatible API release if one isn't present?
    // http://filehost.perforce.com/perforce/r22.2/bin.ntx64/p4api_vs2019_static.zip
    // https://cdist2.perforce.com/perforce/r22.2/bin.ntx64/p4api_vs2019_static.zip
    println!("cargo:rustc-link-search=p4api-2021.1.2179737-vs2017_static/lib/");
    println!("cargo:rustc-link-lib=static=libp4api");

    println!("cargo:rustc-link-lib=crypt32");
    println!("cargo:rustc-link-search=openssl/lib/");
    println!("cargo:rustc-link-search=zlib/lib/");
    println!("cargo:rustc-link-lib=ssleay32");
    println!("cargo:rustc-link-lib=libeay32");
    println!("cargo:rustc-link-lib=Gdi32");
    println!("cargo:rustc-link-lib=User32");

    println!("cargo:rustc-link-lib=Shell32");
    println!("cargo:rustc-link-lib=Ole32");

    println!("cargo:rustc-link-lib=libcmt");
    println!("cargo:rustc-link-lib=oldnames");
    println!("cargo:rustc-link-lib=kernel32");
    println!("cargo:rustc-link-lib=ws2_32");
    println!("cargo:rustc-link-lib=advapi32");

    println!("cargo:rerun-if-changed=include/bridge.h");
    println!("cargo:rerun-if-changed=src/bridge.cc");
    println!("cargo:rerun-if-changed=src/main.rs");

    cxx_build::bridge("src/main.rs")  // returns a cc::Build
        .include("p4/openssl/include")
        .include("p4/zlib/include")// needed?
        .static_crt(true)
        .file("src/bridge.cc")
        .static_flag(true)
        .flag_if_supported("-std=c++14")
        .compile("p4api-bridge");
}

#[derive(Debug)]
enum SetIgnoredError {
    IoError(io::Error),
    NoFileNameError,
}

fn set_directory_as_dropbox_ignored(path : &Path) -> Result<(), SetIgnoredError> {
    let mut s = path.to_path_buf();
    let current_filename = path.file_name().ok_or(SetIgnoredError::NoFileNameError)?;

    let mut f = current_filename.to_os_string();
    f.push(":com.dropbox.ignored");

    s.set_file_name(f);

    let mut file = File::create(s.as_path())?;
    write!(file, "{}", "1").map_err(|e| SetIgnoredError::from(e))
}

impl From<io::Error> for SetIgnoredError {
    fn from(e: Error) -> Self {
        SetIgnoredError::IoError(e)
    }
}