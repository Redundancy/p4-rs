import os
import re

from conans import ConanFile

# Directory containing this recipe (the project root).
_HERE = os.path.dirname(os.path.abspath(__file__))


class P4RsConan(ConanFile):
    """Fetches OpenSSL (and its transitive zlib) for the P4 C++ API build.

    The P4 API is built against one specific OpenSSL, and linking a different one
    is an ABI landmine. Rather than hardcode the version (and hope nobody forgets
    to change it when the SDK moves), we read the OpenSSL version the SDK was
    actually built with -- embedded as a banner string in lib/librpc -- and
    require exactly that. build.rs reads the same banner to pick the matching
    link-library names, so the Conan pin and the linker stay in lockstep.
    """

    settings = "os", "compiler", "build_type", "arch"
    generators = "deploy"

    # Used only if the SDK can't be scanned (e.g. the lib isn't present yet).
    default_openssl_version = "3.0.15"

    def _p4api_dir(self):
        env = os.environ.get("P4API_PATH")
        if env:
            return env if os.path.isabs(env) else os.path.join(_HERE, env)
        # Fall back to scanning. Require a lib/ subdir so an empty leftover like
        # `p4api-extracted/` (from unzip) can't be chosen over the real SDK dir.
        candidates = sorted(
            d for d in os.listdir(_HERE)
            if d.startswith("p4api")
            and os.path.isdir(os.path.join(_HERE, d, "lib"))
        )
        return os.path.join(_HERE, candidates[-1]) if candidates else None

    def _detect_openssl_version(self):
        p4api = self._p4api_dir()
        if not p4api:
            return None
        # librpc.lib on Windows, librpc.a on Linux.
        libdir = os.path.join(p4api, "lib")
        librpc = next(
            (os.path.join(libdir, name)
             for name in ("librpc.lib", "librpc.a")
             if os.path.isfile(os.path.join(libdir, name))),
            None,
        )
        if not librpc:
            return None
        with open(librpc, "rb") as fh:
            blob = fh.read()
        # e.g. b"OpenSSL 1.0.2t  10 Sep 2019" -> "1.0.2t"
        match = re.search(rb"OpenSSL (\d+\.\d+\.\d+[a-z]?)", blob)
        return match.group(1).decode("ascii") if match else None

    def requirements(self):
        # Precedence: explicit override -> detected-from-SDK -> fallback.
        version = os.environ.get("OPENSSL_VERSION") or self._detect_openssl_version()
        if version:
            self.output.info("p4-rs: matching OpenSSL to the SDK -> openssl/%s" % version)
        else:
            version = self.default_openssl_version
            self.output.warn(
                "p4-rs: could not detect the SDK's OpenSSL version; falling back to "
                "openssl/%s. Set OPENSSL_VERSION to override." % version
            )
        self.requires("openssl/%s" % version)

        # OpenSSL pulls in zlib transitively. The old pinned zlib (1.2.12) can no
        # longer be built from source in CI -- its upstream tarball is gone from
        # zlib.net (HTTP 415 on the fossil), and no prebuilt matches the runner's
        # modern toolchain. Override to a current zlib whose source and binaries
        # still resolve; zlib keeps its ABI stable across these versions.
        self.requires("zlib/1.3.1", override=True)
