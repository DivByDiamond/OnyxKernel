# Local heap-code-buffer feature

Upstream: miniz_oxide 0.9.1, https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide
License: MIT OR Zlib OR Apache-2.0 (upstream notices retained).

The public 0.9.1 features have no architecture-independent heap-code-buffer
option. Its compressor holds an inline 64-KiB LZ code array on non-Wasm targets,
which overflows Onyx's 64-KiB kernel stack even when the final compressor is
boxed (the constructor can materialize a temporary). A native RV64 probe
reproduced the canary failure. This feature selects exactly the existing
upstream Wasm boxed-buffer path, using a heap-initialized vector; no algorithm
or public data format changes. Other compressor buffers already use heap-backed
storage in upstream. The fixed native probe verifies actual kernel execution.

The exact change is heap-code-buffer.patch; original published-source file
hashes are UPSTREAM_SHA256.json. Cargo.lock pins adler2 and this crate version.
No registry cache, generated build output or third-party tests are included.

Published crate tar SHA-256: `b63fbc4a50860e98e7b2aa7804ded1db5cbc3aff9193adaff57a6931bf7c4b4c`.
