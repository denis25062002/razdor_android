#!/bin/sh
# Builds the release programs into dist/: `razdor` for Linux (x86_64) and `Razdor.exe` for
# Windows (x86_64). Either one, put into a Discord Times folder (next to DiscordTimes.exe),
# plays that install; elsewhere it uses RAZDOR_DT_DIR (also read from a .env file).
# dist/SHA256SUMS lists their SHA-256: publish it with the files, and check a download with
# `sha256sum -c SHA256SUMS` (Linux) or `Get-FileHash Razdor.exe` (Windows PowerShell).
#
# The Windows build needs the target (`rustup target add x86_64-pc-windows-gnullvm`) and an
# llvm-mingw toolchain (https://github.com/mstorsjo/llvm-mingw, the ucrt build): set
# LLVM_MINGW to its folder, or have x86_64-w64-mingw32-clang on the PATH.
set -eu
cd "$(dirname "$0")/.."
mkdir -p dist

# Panic messages name source files: without the builder's home and folders.
REMAP="--remap-path-prefix=$PWD=razdor --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=cargo --remap-path-prefix=$HOME=~"
RUSTFLAGS="$REMAP" cargo build --release
cp target/release/razdor dist/razdor

if [ -n "${LLVM_MINGW:-}" ]; then
    PATH="$LLVM_MINGW/bin:$PATH"
fi
if ! command -v x86_64-w64-mingw32-clang >/dev/null; then
    echo "x86_64-w64-mingw32-clang not found: set LLVM_MINGW to the llvm-mingw folder" >&2
    exit 1
fi
# crt-static links libunwind in, so the exe needs no DLL of its own. The linker would stamp
# the build time into the exe's header; without it the same commit always gives the same SHA.
CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang \
CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-Wl,--no-insert-timestamp $REMAP" \
CC_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang \
AR_x86_64_pc_windows_gnullvm=llvm-ar \
    cargo build --release --target x86_64-pc-windows-gnullvm
cp target/x86_64-pc-windows-gnullvm/release/razdor.exe dist/Razdor.exe

(cd dist && sha256sum razdor Razdor.exe > SHA256SUMS)
ls -l dist
cat dist/SHA256SUMS
