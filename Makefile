# OnyxKernel - RISC-V 64 kernel workspace (Sv39, 3 rings).
#
# Thin delegation, no duplicated logic. The real commands live in
# .cargo/config.toml (cargo aliases) and the workspace crates.
#
#   make build    kernel ELF via the kbuild cargo alias
#   make smode    kernel for OpenSBI / OC2R S-mode entry
#   make test     host unit tests (onyx_core, onyx_kernel)
#
#   make ibuild   userspace init binaries
#   make tbuild   host tools (mkimage, elf2onx, psfgen)

build:
	cargo kbuild

smode:
	cargo kbuild --features smode

test:
	cargo test -p onyx_core -p onyx_kernel

ibuild:
	cargo ibuild

tbuild:
	cargo tbuild

.PHONY: build smode test ibuild tbuild
