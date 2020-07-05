#!/usr/bin/env sh

set -ex

OS=${1}

export RUST_BACKTRACE=full
#export RUST_TEST_NOCAPTURE=1

rustup update nightly

cargo +nightly install rustup-toolchain-install-master
if [ "${OS}" = "windows" ]; then
    rustup-toolchain-install-master -f -n master -c rustc-dev -c llvm-tools -i x86_64-pc-windows-msvc
else
    rustup-toolchain-install-master -f -n master -c rustc-dev -c llvm-tools
fi
rustup override set master

cargo build
cargo test --verbose -- --nocapture

case "${OS}" in
    *"linux"*)
        TEST_TARGET=x86_64-unknown-linux-gnu cargo test --verbose -- --nocapture
        ;;
    *"windows"*)
        TEST_TARGET=x86_64-pc-windows-msvc cargo test --verbose -- --nocapture
        ;;
    *"macos"*)
        TEST_TARGET=x86_64-apple-darwin cargo test --verbose -- --nocapture
        ;;
esac

# install
mkdir -p ~/rust/cargo/bin
cp target/debug/cargo-semver ~/rust/cargo/bin
cp target/debug/rust-semverver ~/rust/cargo/bin

# become semververver
#
# Note: Because we rely on rust nightly building the previously published
#       semver can often fail. To avoid failing the build we first check
#       if we can compile the previously published version.
if cargo install --root "$(mktemp -d)" semverver > /dev/null 2>/dev/null; then
    PATH=~/rust/cargo/bin:$PATH cargo semver | tee semver_out
    current_version="$(grep -e '^version = .*$' Cargo.toml | cut -d ' ' -f 3)"
    current_version="${current_version%\"}"
    current_version="${current_version#\"}"
    result="$(head -n 1 semver_out)"
    if echo "$result" | grep -- "-> $current_version"; then
        echo "version ok"
        exit 0
    else
        echo "versioning mismatch"
        cat semver_out
        echo "versioning mismatch"
        exit 1
    fi
else
    echo 'Failed to check semver-compliance of semverver. Failed to compiled previous version.' >&2
fi
