"""Conan 2 custom deployer producing a flat dependency layout.

Conan 2 removed the Conan 1 `deploy` generator that produced top-level
`openssl/` and `zlib/` folders. build.rs still expects that flat layout
(`openssl/include`, `openssl/lib`, `zlib/...`), so this deployer reproduces it:
each host dependency's package folder is copied to `<output>/<name>/`.

Invoke with:
    conan install . --deployer=deploy_flat --deployer-folder=. ...
"""

import os

from conan.tools.files import copy


def deploy(graph, output_folder, **kwargs):
    conanfile = graph.root.conanfile
    for _require, dep in conanfile.dependencies.host.items():
        if dep.package_folder is None:
            continue
        dst = os.path.join(output_folder, dep.ref.name)
        copy(conanfile, "*", dep.package_folder, dst)
