#!/bin/sh
# cargo-nextest(MIT または Apache-2.0。開発の道具で、製品には入らない)を入れる。
# クラウドの作業場(Linux x86_64)はコンテナごとに空から始まるので、検査の前に 1 回動かす。
# PC(Windows)の入れ方は docs/verify.md の 1 節。
set -eu
V=0.9.108
if cargo nextest --version 2>/dev/null | grep -q "^cargo-nextest $V"; then
    exit 0
fi
dir="${CARGO_HOME:-$HOME/.cargo}/bin"
tmp=$(mktemp -d)
curl -sSfL "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-$V/cargo-nextest-$V-x86_64-unknown-linux-gnu.tar.gz" |
    tar xz -C "$tmp"
mkdir -p "$dir"
mv "$tmp/cargo-nextest" "$dir/"
rm -rf "$tmp"
cargo nextest --version | head -1
