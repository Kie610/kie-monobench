#!/bin/sh
# cargo-deny(MIT または Apache-2.0。開発の道具で、製品には入らない)を入れる。
# `cargo xtask verify --all` がライセンスの点検に使う。クラウドの作業場(Linux x86_64)は
# コンテナごとに空から始まるので、検査の前に 1 回動かす。PC(Windows)の入れ方は docs/verify.md の 1 節。
set -eu
V=0.19.9
if cargo deny --version 2>/dev/null | grep -q "^cargo-deny $V"; then
    exit 0
fi
dir="${CARGO_HOME:-$HOME/.cargo}/bin"
tmp=$(mktemp -d)
curl -sSfL "https://github.com/EmbarkStudios/cargo-deny/releases/download/$V/cargo-deny-$V-x86_64-unknown-linux-musl.tar.gz" |
    tar xz -C "$tmp"
mkdir -p "$dir"
mv "$tmp/cargo-deny-$V-x86_64-unknown-linux-musl/cargo-deny" "$dir/"
rm -rf "$tmp"
cargo deny --version | head -1
