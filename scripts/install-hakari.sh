#!/bin/sh
# cargo-hakari(MIT または Apache-2.0。開発の道具で、製品には入らない)を入れる。
# `cargo xtask verify` が依存を変えた時と `--all` で、揃えた依存(jc-workspace-hack)の点検に使う。
# クラウドの作業場はコンテナごとに空から始まるので、検査の前に 1 回動かす。PC の入れ方は docs/verify.md の 1 節。
set -eu
V=0.9.39
if cargo hakari --version 2>/dev/null | grep -q "$V"; then
    exit 0
fi
cargo install cargo-hakari --locked --version "$V"
cargo hakari --version
