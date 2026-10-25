<h1 align="center">kieMonoBench</h1>

<p align="center">
  <strong>設計したモデルをそのまま加工に使える、Rust 製のパラメトリック 3D CAD・CAM です。</strong><br />
  スケッチに拘束と寸法を付けて形を作り、同じタイムラインでツールパスまで作ります。寸法を変えると、形もツールパスも作り直されます。
  画面・コマンド(CLI)・AI(MCP)のどれからも、同じ操作ができます。
</p>

<p align="center">
  <a href="https://github.com/Kie610/kie-monobench/releases/latest"><img alt="ダウンロード(Windows・Linux)" src="https://img.shields.io/badge/%E3%83%80%E3%82%A6%E3%83%B3%E3%83%AD%E3%83%BC%E3%83%89-Windows%20%C2%B7%20Linux-2ea44f?style=for-the-badge" /></a>
</p>

<p align="center">
  <a href="https://github.com/Kie610/kie-monobench/releases/latest"><img alt="最新版" src="https://img.shields.io/github/v/release/Kie610/kie-monobench?label=%E6%9C%80%E6%96%B0%E7%89%88" /></a>
  <a href="#ライセンス"><img alt="ライセンス: MIT OR Apache-2.0" src="https://img.shields.io/badge/%E3%83%A9%E3%82%A4%E3%82%BB%E3%83%B3%E3%82%B9-MIT%20OR%20Apache--2.0-blue" /></a>
</p>

<p align="center">
  日本語 · <a href="README.en.md">English</a>
</p>

![AI のエージェントが取付け板を設計してツールパスを作り、パラメータ「幅」を 80 mm から 110 mm に変えて作り直すところ](docs/screenshots/demo.gif)

*穴が 4 つある基板の取付け板を、AI のエージェントが画面を操作して作る様子です。外形の横の寸法をパラメータ「幅」につないであるので、幅を 80 mm から 110 mm に変えると、穴の位置とツールパスも作り直されます。*

![設計の作業空間で、見本の板の押し出しを編集しているところ](docs/screenshots/design.png)

*設計の作業空間です。上に作業空間ごとのツールバー、左にブラウザ、下にタイムライン、右に編集パネルがあります。押し出しの方向を「両方向」にすると、変えた後の形が青くプレビューされます。*

![加工の作業空間で、3D 粗取りの欄を直しながらツールパスをプレビューしているところ](docs/screenshots/machining.png)

*加工の作業空間です。左のセットアップの木に、設計の形から作った加工が並びます。3D 粗取りの欄を直すとツールパスが作り直され、安全高さ・送り高さ・トップ高さ・ボトム高さの札と一緒に表示されます。*

## ダウンロード

Windows 10・11(x64)と Linux(x86_64)向けのアプリを、[最新のリリース](https://github.com/Kie610/kie-monobench/releases/latest)からダウンロードできます。展開すればそのまま起動でき、インストールは要りません。

アプリにはまだ署名がありません。Windows では、初めて起動した時に警告のダイアログが出ることがあります。

まだ開発の途中です。できないことと今後の予定は [docs/できないことと予定.md](docs/できないことと予定.md) にあります。

画面のアプリは `kie-monobench`、コマンドは `kmb`、MCP サーバは `kmb-mcp` です。文書の拡張子は `.kmb` で、中身は JSON です。前の仮の名前 `jisaku-cadcam` のデータフォルダは最初の起動で引き継ぎ、拡張子が `.json` の文書もそのまま開けます。

## できること

| 分野 | できること |
|---|---|
| スケッチ | 線分・長方形・円・円弧・長穴・多角形・楕円・スプライン・円錐曲線・文字を描き、拘束と寸法で決めます。寸法はパラメータの式で書けます。3D スケッチと下絵(キャンバス)もあります |
| 形を作る | 押し出し・回転・穴とねじ・スイープ・ロフト・フィレット・面取り・シェル・ブーリアン・ボディの分割・移動とコピー・面のオフセット・パターン・ミラーがあります。どれもタイムラインに並び、後から直せます |
| パラメータと構成 | 単位つきの式(mm・in など)、パラメータの表、寸法の組を切り替える構成表があります |
| 組立 | 部品、ジョイント、剛性グループ、モーション スタディ、接触セット、干渉の確認、規格部品、部品表があり、ほかのファイルをリンクで挿入できます |
| 加工(CAM) | セットアップでストック・原点・固定具を決めます。2D の加工は負荷制御粗取り・ポケット・フェイス・輪郭・スロット・彫刻・V 彫り・面取り・ねじ切り・穴あけ、3D の加工は 3D 粗取り・走査線・等高線・スキャロップ・ペンシル・渦巻き・放射状などです。レーザー加工もできます |
| 複数の工具 | 複数の工具を使えます。前の工具が届かなかった所は、細い工具で削れます |
| 確かめる | 切削シミュレーションで、食い込み・削り残し・ホルダーの当たりを調べられます。書き出した G-code を読み直して削る確認と、加工指示書(Markdown・HTML)の書き出しもできます |
| レンダリングとアニメーション | 外観・光源・デカールを付け、静止画と回転する動画、分解とジョイントの動きの動画を書き出せます |
| 解析 | 強度と熱、流れと空力を解けます |
| ファイルと版 | 文書はローカルの JSON で、ネットワークを使いません。保存のたびに版が残り、前の版と見比べて戻せます |
| 画面・CLI・MCP | どれも同じ操作の宣言と確認を通ります。操作と道具の一覧は [docs/入出力.md](docs/入出力.md) にあります |

## 対応する機械とファイル形式

| 種類 | 対応 |
|---|---|
| 機械(G-code のポスト) | Carvera Air(既定)、汎用 3 軸(Fanuc 系・Grbl 系)、SCULPFUN G20(ガルバノのレーザー)。WASM のポストのプラグインも足せます |
| 読み込み | STEP、DXF、SVG、画像(下絵) |
| 書き出し | STEP、STL、3MF、glTF(.glb)、今の視点の DXF・SVG、PNG と動画、G-code、加工指示書、部品表の CSV |
| 文書 | JSON。古い形式は 1 段ずつ変換して読みます。新しい形式の文書は読まずに止めます |

加工の核は機械に依存しません。機械ごとの違いは、マシンライブラリとポストだけにあります。

## ソースからビルドする

ビルドには次の物が要ります。

- Rust が要ります。版は `rust-toolchain.toml` で決まっていて、rustup が自動で入れます。
- C++17 のコンパイラが要ります。Windows は Visual Studio 2022 の MSVC、Linux は g++ です。形状カーネル SMLib(`third_party/usd-brep`)のビルドに使います。CMake は要りません。初回のビルドには、4 コアで 3〜8 分かかります。
- Linux で画面を動かす場合は、X11 か Wayland と、Vulkan のドライバが要ります。

```sh
cargo run --release -p kmb-app -- --sample   # 見本の部品と加工で画面を開く
cargo run --release -p kmb-app               # 開始画面から始める
cargo run --release -p kmb-cli -- --help     # コマンド(kmb)の一覧
cargo xtask verify                          # 変えた crate のテストと点検
```

形状カーネルの既定は SMLib です。環境変数 `JC_KERNEL=truck` で truck に替えられます。SMLib を入れずにビルドする時は、アプリを `--no-default-features` でビルドします。この場合のカーネルは truck だけです。

検査の手順とログの決まりは [docs/verify.md](docs/verify.md) にあります。

## 使い始める

### 画面

引数なしで起動すると開始画面が出ます。開始画面には見本と「はじめての手順」があり、見本を開いてから G-code を書き出すまでを順に試せます。操作の流れは [docs/使い方.md](docs/使い方.md) にあります。

### コマンド(`kmb`)

文書を作る、パラメータを変える、機能を足す、ファイルを書き出す、G-code を作る、削ってみる、といった操作ができます。画面のボタンと同じ操作は `kmb actions` で一覧でき、`kmb run` で実行できます。

```sh
kmb sample --cam cam.kmb        # 見本の部品と加工の文書を作る
kmb param set cam.kmb width "60 mm"
kmb gcode cam.kmb cam.nc        # Carvera Air 向けの G-code
kmb sim cam.kmb                 # 食い込みと削り残しを調べる
kmb export cam.kmb part.step
```

### AI(MCP)

`kmb-mcp` は標準入出力で動く MCP サーバです。読み書きできるのは、作業フォルダ(`--root`)と `--allow` で許したフォルダだけです。画面の設定で「AI との接続」を入れると、動いている画面の文書も操作できます。

```json
{ "mcpServers": { "kiemonobench": {
  "command": "<リポジトリの場所>/target/release/kmb-mcp",
  "args": ["--root", "<読み書きを許すフォルダ>"] } } }
```

前の名前(コマンド `jc`、MCP サーバ `jc-mcp`)から移る時は、次の 2 つを直してください。

1. MCP の設定の `command` を `.../kmb-mcp`(Windows は `kmb-mcp.exe`)に書き換えます。
2. スクリプトで `jc` と打っていた所を `kmb` にします。

設定のキー(上の `kiemonobench`)は変えなくてかまいません。前の名前の実行ファイルはもうビルドされないので、`target/release` に残った `jc` と `jc-mcp` は消してかまいません。ログのファイル名の先頭は `kmb-cli-` と `kmb-mcp-` です。

### ポストのプラグイン

ポストは WASM のコンポーネントとして足せます。インタフェースは `crates/kmb-plugin-host/wit/post.wit` です。プラグインはファイル・ネットワーク・時計に触れられません。見本は `plugins/post-example/` にあります。

### 実機で削る前に

実機で削る前に、必ず次の順で確かめてください。シミュレーション、Z を浮かせた空運転、ワックスや MDF での試し削りの順です。

## 開発に加わる

| 知りたいこと | 資料 |
|---|---|
| Issue と PR の出し方 | [CONTRIBUTING.md](CONTRIBUTING.md) |
| 脆弱性の知らせ方 | [SECURITY.md](SECURITY.md) |
| 参加する人の振る舞い | [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) |
| 思想・構造・入出力の約束・見た目と作業の決まり | [AGENTS.md](AGENTS.md) と [.claude/rules/](.claude/rules/) |
| どこに何があるか | [docs/地図.md](docs/地図.md) |
| 方針・段階と設計 | [docs/policy.md](docs/policy.md)、[docs/plan.md](docs/plan.md) |
| 画面の言葉 | [docs/用語表.md](docs/用語表.md) |
| 機能ごとの記録 | [docs/機能/](docs/機能/) |

決まりは `cargo xtask verify` の点検で守ります。GPL・LGPL・AGPL の依存とコードは使いません。ほかのソフトのコードは写しません。特許のある操作の形は作りません。作らない形は [docs/避ける形.md](docs/避ける形.md) にあります。

## ライセンス

MIT([LICENSE-MIT](LICENSE-MIT))と Apache-2.0([LICENSE-APACHE](LICENSE-APACHE))のどちらかを、使う人が選べます。著作権者の表記は「kieMonoBench contributors」です。同梱する他のソフトとデータは、それぞれのライセンスに従います。

このソフトは独立に作っています。機械や CAD・CAM の製品の作り手とは関係がありません。機械・ファイル形式・ソフトの名前はそれぞれの持ち主の商標で、互換を示すためだけに書いています。

製品名「kieMonoBench」とアプリのアイコンは、コードのライセンスに入りません。使ってよい範囲と、手を加えた版を配る時の決まりは [TRADEMARKS.md](TRADEMARKS.md) にあります。

### 同梱する他のソフトとデータ

| 置き場 | 中身とライセンス |
|---|---|
| `third_party/usd-brep/` | NVIDIA usd-brep 1.0.0 の SMLib と SM_API。Apache-2.0、Copyright (c) 2020-2026 NVIDIA CORPORATION & AFFILIATES。全文は `third_party/usd-brep/LICENSE` |
| `third_party/smlibpy-patches/` | smlibpy の SMLib の修正 2 つ。MIT、Copyright (c) 2026 AWV Informatik AG。全文は `third_party/smlibpy-patches/LICENSE`。`crates/kmb-kernel-smlib/shim/jc_step.cpp` の STEP の書き出しも、smlibpy の書き出しを元にしています |
| `third_party/truck-shapeops/` | truck-shapeops 0.4.0 を直した写し。予備の形状カーネル truck のブーリアンに使います。Apache-2.0、Copyright (c) RICOS Co. Ltd.。全文は `third_party/truck-shapeops/LICENSE` |
| `crates/kmb-text/fonts/` の `futural.jhf` と `rowmans.jhf` | Hershey フォントのデータ。全文は `crates/kmb-text/fonts/HERSHEY-LICENSE.txt` |

Hershey フォントの許諾が求める表記は次のとおりです。

> The Hershey Fonts were originally created by Dr. A. V. Hershey while working at the U. S. National Bureau of Standards. The format of the Font data in this distribution was originally created by James Hurt, Cognition, Inc.

出どころと更新の手順は [third_party/README.md](third_party/README.md) にあります。
