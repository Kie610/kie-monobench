# third_party

外から持ってきたソースです。ここにあるものは、ここに書いた以外は手を加えていません。

## usd-brep/(SMLib と SM_API)

- 出どころ: https://github.com/NVIDIA-Omniverse/usd-brep の 1.0.0(commit c9c5979)
- 入れたもの: `source/SMLib/{inc,src}` と `source/SM_API/{inc,src}` だけ(premake5.lua は除いた)、`LICENSE`、`VERSION`
- 入れていないもの: OCCT(LGPL)を使う STEP の読み書き(`OCCT_BREP_*`)、OpenUSD を使う部分、Python の GUI
- 許諾: Apache License 2.0(`usd-brep/LICENSE`)。Copyright (c) 2020-2026, NVIDIA CORPORATION & AFFILIATES.
- ビルド: `crates/smlib-sys` が cc で静的ライブラリにする

## smlibpy-patches/

- 出どころ: https://github.com/awv-informatik/smlibpy の commit 3695509、`third_party/patches/SMLib/src`
- 中身: SMLib の 2 つのファイル(`SmVertex.cpp`、`SmProtoTopology.cpp`)の不具合を直した写し。ビルドでは同じ名前の元のファイルの代わりに使う
  (1.0.0 のブーリアンは、これが無いと解放済みのメモリに書き込む)
- 許諾: 写しそのものは元と同じ Apache License 2.0(変えた所はファイルの先頭と `smlibpy patch` の注記)。smlibpy は MIT(`smlibpy-patches/LICENSE`)
- 直した中身の説明: `smlibpy-patches/README-upstream.md`(smlibpy の原文)

## truck-shapeops/(truck のブーリアン)

- 出どころ: crates.io の truck-shapeops 0.4.0(https://github.com/ricosjp/truck 。上流の master 78d699b、2026-10-19 も同じ所は変わっていない)
- 入れたもの: `src`、`README.md`、`Cargo.toml`(例と開発用の依存は外した)、`LICENSE`(上流のもの)
- 許諾: Apache License 2.0(`truck-shapeops/LICENSE`)。Copyright (c) RICOS Co. Ltd.
- 使い方: ワークスペースの `Cargo.toml` の `[patch.crates-io]` で crates.io の物の代わりに当てる(Kie 2026-10-20 承認)。ワークスペースの部品にはしない(`exclude`)
- 変えた所(印 `jisaku-cadcam:`)は 2 つ。1 つ目は `src/lib.rs` の `#![cfg_attr(not(debug_assertions), deny(warnings))]` を外したこと。crates.io の依存は `--cap-lints` で警告が抑えられるが、パスで当てた写しは抑えられない。そのため release のビルドだけで、上流にもある「使われていない `Alternative`」がエラーになった(2026-10-20、PC のアプリの作り直しで発覚)。2 つ目は `src/transversal/integrate/mod.rs` の `ShellRegion`。相手と交わらない殻の面の内外を光線で数える時、上流は網(曲面の向きのまま)だけで数えるので、`not()` で向きを裏返した殻(引き算の道具、向きを直した回転体)を逆に数え、内向きの殻(空洞)を中身と読んでいた。面の向きに沿って数え、内向きの殻は外側を中身とする。これが無いと、中に浮いた道具を引く(空洞を作る)と内部エラーか空の結果になり、空洞の横の切り取り・交差も壊れる(試験は jc-contract の `boolean_beside_a_sealed_void_keeps_the_body`・`revolved_body_with_a_void_cuts_beside_it`)
- `Cargo.toml` の truck-meshalgo は既定の機能 "vtk" を外した(印 `jisaku-cadcam:`)。VTK 形式の読み書きは使わず、入れると古い quick-xml と lz4_flex(既知の脆弱性あり)が入るため(2026-10-25)
- 外す時: 上流が同じ直しを入れた版が出たら、この写しと `[patch.crates-io]` を消し、`cargo test -p jc-kernel-truck` を通す

## 更新のしかた

1. usd-brep の新しい版の `source/SMLib` と `source/SM_API` の `inc`・`src` で置き換える
2. 上流で直った不具合の写しを `smlibpy-patches/` から消す(`README-upstream.md` の「Dropping a patch」)
3. `cargo test -p jc-kernel-smlib` と、ワークスペース全体の試験を通す
