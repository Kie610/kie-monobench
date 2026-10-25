# 部品ごとの検証とログ

モジュール(crate)を直すたびに全体をビルドし直さず、変えた所だけを確かめる手順と、何かあった時に追えるログの使い方を書く。

## 1. 変えた crate だけを検査する

| やりたいこと | コマンド |
| --- | --- |
| 変えた crate のテストと clippy(作業中はこれ) | `cargo xtask verify` |
| crate を名指しで検査する | `cargo xtask verify kmb-ui kmb-cam-commands` |
| 変えた crate に依存する側も含める(プッシュの前。PC での取り込みの検査もこれ) | `cargo xtask verify --dependents` |
| 全体を検査する(クラウドで手渡す前。前後に下の `cloud-target-cache.sh`) | `cargo xtask verify --all` |
| 全体を、truck のカーネルでも検査する(公開の準備の時。週に 1 回は `--all` が自動で足す) | `cargo xtask verify --all --truck` |
| 何が対象になるかを見るだけ | `cargo xtask affected [--dependents]` |
| 組み立てずに点検だけを流す(数秒。下の「点検」) | `cargo xtask checks` |
| 機能の断片から一覧の表を作り(`target/docs/`)、地図.md の crate の一覧を書き直す | `cargo xtask docs` |
| 入出力の一覧 docs/入出力.md(操作と引数・役割・設定・文書の版・終了の番号・MCP の道具)を登録から書き直す。古いと kmb-mcp の試験が落ちる | `cargo xtask io` |
| コアの公開の項目の一覧を見る | `cargo xtask api` |
| target の、今の版で使わない成果物を見積もる・消す(8 節) | `cargo xtask sweep` / `cargo xtask sweep --yes` |
| アプリを release で作り、exe にこの PC のパスが残っていないかを数える(12 節) | `cargo xtask release` |
| 配るファイルに残るこの PC のパスを数える(12 節) | `cargo xtask paths <ファイル>...` |
| 配る exe に入る他者の部品の許諾文の一覧を作る(13 節) | `cargo xtask licenses` |

- 「変えた」は `origin/main` との分かれ目からの差で、まだコミットしていないファイルと git に入れていないファイルも含む。比べる相手は `--base <ref>` で変えられる。
- 根の `Cargo.toml`・`Cargo.lock`・`.cargo/` を変えた時は、全体を検査する。`docs/` など crate の外のファイルはテストしない(何を飛ばしたかは表示する)。ただし下の点検はいつも流し、テストの段は「未実行」と出す(通過ではない)。
- `--all`・`--dependents` は cargo-nextest が無ければ失敗にする(件数が残らないため。nextest なしで流すなら `JC_VERIFY_NEXTEST=off`)。
- 最後に段ごとの「通過/失敗」とかかった秒数を出す。失敗があれば終了コードが 1 になる。
- clippy は警告を誤りとして扱う(`-D warnings`)。テストだけなら `--no-clippy`。
- 普段の検査(手渡しの前・PC での取り込みを含む)は SMLib のカーネルだけで流し、`--truck` は付けない。truck の回は、公開の準備で truck のカーネルも配る前と、週に 1 回だけ流す(使うのは SMLib だけで、truck は戻すための控え)。
  - 週に 1 回: `--all` は、検査の時間の記録(この節の下の `history.md`)で truck の回が最後に通ってから 7 日を過ぎていれば、`--truck` を付けたのと同じに truck の回も流す(`xtask/src/history.rs` の `truck_due`)。truck の回は SMLib の回と同時に動くので、全体の時間はほぼ延びない。SMLib だけで作る新しい形の操作の試験が、truck の飛ばしに入らないまま溜まるのを防ぐ(10-18 から 1 週間 truck の回を流さず、落ちる 12 件と表に無い飛ばし 17 件が溜まった)。記録はクラウドでは共有フォルダにあるのでコンテナをまたいで数え、PC では `target/verify/history.md` で数える。
  - truck の回で落ちたら、1 件ずつ truck の弱さか JC の退行かを確かめる(SMLib の回では通るか、truck の回で前に通っていたか、落ちた所の形)。退行なら直す。truck の弱さなら、試験の初めに `if k.is_truck() { kmb_contract::skip!("truck は…") }` と何が作れないかを書き、`xtask/skips.toml` の `[runtime]` に試験の名前と「truck の回(…)」を足す。新しい形の操作の試験を足す時も、truck で作れなければ同じに書く。
  - truck に頼っている所: SMLib の組み立てでも、STEP の読み込みは truck に任せている(`kmb-bundle` の `kernel_of`、`SmlibKernel::with_step_import`)。この読み込みの試験(`kmb-kernel-truck` 自身の試験と、`kmb-kernel-smlib`・`kmb-ui`・`kmb-samples` の STEP を読む試験)は SMLib の回にも入っているので、普段の検査で確かめ続ける。truck で形を作る処理(押し出し・ブーリアンなど)だけが普段の検査から外れる。
- `--truck` の 2 回目(truck の回)は、カーネルの違いで結果が変わり得る形の計算の試験だけを流す: `kmb-solid` の全部と、`kmb-samples` の穴の見分けの試験(`hole_tests::`)。どこを流すかは `xtask/src/main.rs` の `TRUCK_TESTS`。形を作った後の処理(加工・画面の操作・MCP・見本の加工)の試験はどちらのカーネルでも同じ中身になるので、速い方の SMLib の回だけで流す(同じ試験が truck では 3.5〜15 倍遅い。6 節)。形の計算の試験を新しい crate やモジュールに足した時は、`TRUCK_TESTS` にも足す。
- 道具は `xtask/`(ワークスペースの 1 crate)で、`.cargo/config.toml` の別名 `xtask` から動く。

### 点検(組み立てずに毎回流す)

`cargo xtask verify` は、どの crate を選んでも(docs だけの変更で何も選ばなくても)、最初に組み立てずに次を点検する(数秒)。試験の中に置くと、その crate を選ばない変更(画面・CLI・MCP の crate だけを変えた時など)で流れないため。

- 層(`xtask/src/layers.rs`): 各 crate の `[package.metadata.kmb] layer` と、`cargo metadata` の依存の表(通常・ビルド用・テスト用)。テスト用の依存は、通常の表に足して試験の部品(test)と組み立て(assembly)を使ってよく、カーネルは kmb-solid とカーネル・組み立てだけ。画面の拡張(kmb-ui-*)は外枠の部品と、扱う種類のモジュール(`UI_EXTENSIONS`)だけ。他の層の crate を `pub use` で出し直さない(コアは除く)。
- 他の層の名前: 製品のソースの文字列に、他の層のモジュールが名乗る名前(機能の種類・操作・設定の ID)を書かない。名前の持ち主の表は `xtask/names.txt` で、kmb-bundle の試験 `tests/ids.rs` が登録から書く(登録を変えたら `JC_BLESS=1 cargo test -p kmb-bundle --test ids`)。`#[cfg(test)]` は付いた項目の範囲だけ飛ばす。
- 飛ばし(`xtask/src/skips.rs`): 理由の無い `#[ignore]`(`#[ignore = "道具: …"]` のように書く)と、`#[ignore]` でないテストの本体の `return` を落とす。環境が無くて飛ばすテストは `kmb_contract::skip!("理由")` を使う。印が `target/verify/skips.log` に残り、まとめに「実行中の飛ばし N 件」と名前つきで出る。例外は `xtask/skips.toml`(`[static]` は return してよいテスト、`[runtime]` は実行中に飛ばしてよいテスト)。直って要らなくなった行が残っていても落ちる。 テストのコードで、決まった回数だけ回して `sleep` し、揃えば `return;`・`break;` で抜け、回し終えた後に `assert!`・`panic!` の無い待ちも落とす(待ちきれずに先へ進み、負荷の高い時だけ揺れる。時間切れで誤りにして止める)。
- 消えたテスト: `--base` との分かれ目から消えたテスト(変えたファイルを、分かれ目の版と今の版で比べる)。消してよいのは `xtask/removed-tests.toml` に日付と理由を書いた物だけ(確認は要らない。消した物は区切りの報告に一覧で書く)。同じ名前のテストが別のファイルや別の crate へ移っただけの物は消えたことにしない(crate の分割)。丸ごと消した crate のファイルも数える。マクロが作るテストは数えない。
- 資料(`xtask/src/docs.rs`): docs/機能/ の断片の名前と頭の欄と節(docs/機能/README.md)。plan.md「決めたこと」に表や箇条の行を足した、断片に特許の点検の記録(頭の「特許の判定」「確定度」、節「特許の点検」)を書いた、を落とす。`xtask/names.txt` に名前が増えたか crate を足したのに、新しい断片が無いのも落とす(新しい機能の PR は断片を必ず 1 つ足す)。docs の取り込みの衝突の印と、見出しと区切りの行で始まらない表(描かれない)も落とす。docs/地図.md の crate の一覧が Cargo.toml と合わなければ落とす(`cargo xtask docs` で作り直す)。
- 骨格の資料(`xtask/src/agents.rs`): 直下の AGENTS.md と .claude/rules/ の分野の決まり。コードの印(`…`)に書いたパス・crate・型や関数の名前・xtask の命令と、「policy.md「節」」の節が無ければ落とす(名前を変えた PR が資料を直し忘れるのを防ぐ)。大きさの上限(AGENTS.md 12 KB、分野の決まり 1 つ 4 KB)、分野の決まりの `paths:` が無いか当たらない、CLAUDE.md が `@AGENTS.md` を読まない、も落とす。層の表は生成区画で、`cargo xtask docs` が `layers.rs` から作る(古いと落ちる)。決まりの行に付けた「(点検: 名前)」は `agents.rs` の `CHECKS` の名前で、その点検の所に印の字が無ければ落とす。点検の無い決まりと決まりの無い点検は落とさず数を出し、`cargo xtask docs` が一覧を出す。比べる前に CRLF を LF に直す。
- 変えたら直す合図(`agents.rs` の `change_hints`): 分かれ目から層の表か docs/入出力.md の役割・文書の版・終了の番号・MCP の道具の節が変わったのに、AGENTS.md の本文(層の表の外)が変わっていなければ「合図:」を出す。落とさない。
- 画面の再計算(`xtask/src/layers/code.rs` の `screen_recomputes`): 画面とアプリの層で `recompute_watched`・`take_cache`・`hold_shapes` を呼ばない(再計算は kmb-session の Headless を通す)。
- コアの公開の項目(`xtask/src/api.rs`): crate を名指ししない時、kmb-core の公開の項目(`pub` の頭、`pub enum` の値、`pub trait` の項目。字面で読む)を分かれ目と比べ、変わっていれば契約テスト(kmb-contract)を使う crate を全部足して流す。項目は「ファイル: 項目」で数えるので、ファイルを分けただけでも変わったと見なす。`CORE_API_VERSION` は公開まで上げない(PR ごとに上げていた版は誰も読まず、衝突の元だった)。
- 組み立ての名前の一意(kmb-bundle の試験 `tests/ids.rs`): 操作の ID・役割の印・設定の ID が重ならず、`then`・`before`・段の帯の操作が実際にある操作を指す。見本の文書 3 つで、何も選ばない時と機能を 1 つずつ選んだ時の操作を集める。

ライセンス(`--all` の時と、`Cargo.lock`・`Cargo.toml`・`deny.toml` を変えた時): `cargo deny check licenses bans sources`(`deny.toml`)と、Windows 版の許諾文の一覧が作れること(13 節)。`--all` では cargo-deny が無いのも失敗。入れ方(版 0.19.9): クラウドは `scripts/install-deny.sh`、PC は `cargo install cargo-deny --locked --version 0.19.9`(`scripts/取り込み.ps1` は無ければ入れる)。依存の表は作る環境(Windows と Linux)で組み立てる物だけを見る。

揃えた依存(`--all` の時と、`Cargo.lock`・`Cargo.toml`・`deny.toml` を変えた時): `cargo hakari generate --diff` と `cargo hakari manage-deps --dry-run` が差を出せば落とす。cargo-hakari が無いのも失敗(依存を変えたのにクラウドで作り直さず、PC の取り込みで初めて落ちるのを防ぐ)。差が出たら `cargo hakari generate`・`cargo hakari manage-deps --yes` で直し、`plugins/post-example` で `cargo metadata` を流してその Cargo.lock も合わせる(kmb-plugin-host の試験が比べる)。入れ方(版 0.9.39): クラウドは `scripts/install-hakari.sh`、PC は `cargo install cargo-hakari --locked --version 0.9.39`。

### 時間の記録

`cargo xtask verify` は 1 回ごとに、日時・場所・コミット・指定・結果と、段ごとの秒数(組み立てはコンパイルした crate の数、テストは通過・失敗・飛ばしの件数つき)を Markdown の表の 1 行として足す(`xtask/src/history.rs`)。

- クラウドの作業場: `/mnt/project-files/ops/verify/history.md`(コンテナをまたいで残り、どの作業の回も並ぶ。`.cargo/config.toml` の `JC_VERIFY_HISTORY`)。
- PC など `/mnt/project-files` の無い所: `target/verify/history.md`(`cargo clean` で消える。`cargo xtask sweep` は消さない)。
- 書けなくても検査は止めない(知らせるだけ)。テストごとの時間は前と同じく nextest の `junit.xml`(下)。

### クラウドで組み立て済みの target を使い回す

クラウドの作業場はコンテナごとに target が空から始まり、`--all` のたびに一から組み立てていた(組み立て 614 秒、clippy 402 秒)。組み立て済みの `target/debug` を共有フォルダに固めて置き、次のコンテナで戻す。

| 場面 | コマンド |
| --- | --- |
| 検査の前(コンテナで 1 回。cargo-nextest と cargo-hakari は無ければ verify が `scripts/install-*.sh` で入れる) | `scripts/cloud-target-cache.sh restore` |
| 手渡し前の `--all` が通った後 | `scripts/cloud-target-cache.sh save` |

- 置き場所は `/mnt/project-files/.cache/target/debug-linux-<UTC の時刻>/part-NNN`(1 組だけ。固めて 256 MB ずつに分ける。2026-10-22 は 19 GB → 5.5 GB・22 個)と、作った版と欠片の数を書いた `debug-linux.txt`。`save` は `cargo xtask sweep --yes` で今の版が使わない物を除いてから固める。共有フォルダはプロジェクトで 10 GiB まで(`JC_SHARED_QUOTA_GIB`)で、2026-10-23 は 9 GiB 使っていたので、前の組を残したまま書くと上限に当たる。そこで、使っている量 − 前の組 + 新しい組の見込み(前の組の 1.1 倍)が上限に収まるかを先に確かめ(収まらなければ置かない)、前の組と `debug-linux.txt` を消してから書き、書き終わってから `debug-linux.txt` を作る。書き込みに失敗したら書きかけを消す。消してから書き終わるまでの間に `restore` したコンテナは戻さず、一から組み立てる。`restore` は欠片の数が合わなければ戻さない。
- 別のコンテナと譲り合う: `restore` は読む間 `readers/` に印を置く。`save` は先に `debug-linux.txt` を外して新しい `restore` を止め、読んでいる印が消えるのを待ってから(`JC_CACHE_WAIT_S`、既定 900 秒。過ぎたら置かない)前の組を消す。`save` どうしは `saving/` の錠で 1 つだけ。`restore` は `target/.debug-restoring` へ取り出してから移すので、途中で切れても半端な `target/debug` を残さない。
- コンテナでは cargo より先に `restore` を流す。先に cargo を流すと `target/debug` ができて、`restore` が戻さずに終わる。
- 分けるのは、共有フォルダが 1 ファイルおよそ 0.5 GB を超えると上げず、書いたコンテナが終わると消すから(2026-10-22 に測った。400 MB は 20 秒で上がり、700 MB と 1 GB は 7〜25 分たっても上がらなかった)。前の 1 ファイル(4.4 GB)は一度もコンテナをまたいで残っていなかった。上がるまでの間はメモリ(`/dev/shm`)に載る。
- `restore` は `target/debug` が無い時だけ戻す。Rust の版が違えば戻さない。戻した後に空きが 10 GB 残らなければ戻さず(戻す大きさを書いていない古い組は、固めた大きさの 4 倍と見る)、揃えた依存(`crates/kmb-workspace-hack/Cargo.toml`)が置いた時と違えば incremental を戻さない(使われないので。8 節)。戻した target は cargo が中身を確かめて使うので、古い版の物でも結果は変わらない(違う所だけ作り直す)。取り出したばかりのソースは時刻が新しいので、自前の crate は毎回作り直す(incremental が残っているので速い)。そこで自前の crate の成果物(テストと実行ファイル、lib の `.rlib`・`.rmeta`、`target/debug` 直下の実行ファイル)は `save` で置かず、`restore` でも戻さない(型は `cargo metadata` の crate の名前から作る)。2026-10-23 は戻す量が 18 → 8.9 GB、戻した後の空きが 12.7 → 20.8 GB になった(8 節)。
- zstd が無ければ apt で入れる。`/mnt/project-files` の無い所(PC)では何もしない。

### テストの流し方(cargo-nextest)

[cargo-nextest](https://nexte.st/)(MIT または Apache-2.0。開発の道具で、製品には入らない)があれば、xtask はテストを nextest で流す。nextest はテストを 1 本ずつ別のプロセスにし、crate(テストの実行ファイル)をまたいで CPU の数だけ並べる。前の `cargo test` は実行ファイル(約 60 本)を 1 本ずつ順に流し、しかも SMLib を呼ぶテストは錠(`kmb-kernel-smlib` の `ffi.rs` の `SMLIB`)で同じプロセスの中では 1 本ずつになっていた。

1. 組み立て: `cargo nextest list --list-type binaries-only` でテストの実行ファイルを 1 回だけ作る。
2. SMLib の回・truck の回・clippy を同時に動かす。テストの回は 1 の実行ファイルを使い回し(`--binaries-metadata`)、cargo を呼ばないので、clippy と target の錠を取り合わない。

- 段ごとの出力はログ(`target/verify/test.log`・`test-truck.log`・`clippy.log`)へ書き、終わった順に結果を表示する。テストの段は nextest の集計(60 秒を超えたテストの一覧を含む)を表示する。失敗したテストは集計の中に出力つきで並ぶ。ログは毎回消してから作り、集計が 2 本ある時と、終了は通過なのに集計に失敗がある時は段を失敗にする(止めた前の検査の nextest が同じログに書いた疑い)。verify は `target/verify.lock` の錠を取り、同じ target で同時に 2 つ動かさない。
- テストごとの時間: `target/nextest/verify/junit.xml`(SMLib の回)・`target/nextest/verify-truck/junit.xml`(truck の回)。設定は `.config/nextest.toml`。
- nextest は doctest を流さない。説明のコメントに試験として動くコードの囲みがある crate があれば、clippy の前に `cargo test --doc` で流す(2026-10-17 の時点では 1 つも無い)。
- プロセスが別なので、テストの間で `static` や `thread_local!` は共有されない(各テストは前から自分で準備を作り直していたので、結果は変わらない)。
- 入れ方(版は 0.9.108):
  - クラウドの作業場: `scripts/install-nextest.sh`(コンテナごとに 1 回。GitHub のリリースから取る。数秒)。
  - PC(PowerShell): 出来合いを取るなら `Invoke-WebRequest https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.108/cargo-nextest-0.9.108-x86_64-pc-windows-msvc.zip -OutFile $env:TEMP\nextest.zip; Expand-Archive $env:TEMP\nextest.zip -DestinationPath $env:USERPROFILE\.cargo\bin -Force`(数秒)。手元で作るなら `cargo install cargo-nextest --version 0.9.108 --locked`(数分コンパイルする)。
  - 入ったかは `cargo nextest --version`。
- 無い時は前と同じ `cargo test` で 1 つずつ流す(遅い。そう表示する)。`JC_VERIFY_NEXTEST=off` でも前の形になる。

## 2. 依存を作り直さないための crate(`kmb-workspace-hack`)

Cargo は、選んだ crate の組み合わせごとに依存先の機能(features)を決め直す。そのため `cargo test -p kmb-cam-commands` のように一部だけを選ぶと、全体のビルドと機能の組み合わせが変わり、wasmtime や egui などの依存先を最初から作り直していた(測った例: `-p kmb-cam-commands` で 2 分 46 秒、`-p kmb-ui` で 6 分 15 秒)。

`crates/kmb-workspace-hack` は中身のない crate で、全部の crate がこれに依存し、依存先の機能を全体と同じにそろえる。入れた後は、どの crate を選んでも依存先の作り直しは 0 件だった(`kmb-cam-commands`・`kmb-ui`・`kmb-expr`・`kmb-sketch` などで確認)。

- 作るのは [cargo-hakari](https://docs.rs/cargo-hakari)(MIT または Apache-2.0。開発の道具で、製品には入らない)。設定は `.config/hakari.toml`。
- 入れた直後の 1 回だけ、全体のビルドで依存先を作り直す(3 節の変更と合わせ、クラウドで約 14 分)。
- **依存を足した・外した時、crate を足した時**は、次を動かして結果もコミットする。

  ```text
  cargo install cargo-hakari --locked   # 初回だけ
  cargo hakari generate                 # 機能のそろえ直し
  cargo hakari manage-deps --yes        # 新しい crate に kmb-workspace-hack への依存を足す
  cargo hakari verify                   # そろっているか確かめる
  ```

  忘れても壊れはしない(ビルドとテストは通る)。一部の crate を選んだ時に、また依存を作り直すようになるだけ。

## 3. 依存先のデバッグ情報は行番号だけ

`Cargo.toml` の `[profile.dev.package."*"]` に `debug = "line-tables-only"` を置いた。依存先(egui、wgpu、wasmtime など)の変数の型などの詳しい情報を持たず、行番号だけを持つ。

- 自前の crate は今まで通り詳しい情報を持つ。パニックやクラッシュ報告の逆追跡には、依存先の中でもファイルと行が出る。
- デバッガで依存先の中の変数を見たい時だけ、一時的に外す。

## 4. 画面の部品の確かめ方

- 画面の動き(ボタンを押すと文書が変わる、欄に出る値など)は、GPU なしで動く描画テスト(egui_kittest)で確かめる。`cargo xtask verify kmb-ui`(`kmb-ui-sketch`・`kmb-ui-tools` も同じ)。
- 描いた内容を SVG に、3D のビューを BMP にする: `JC_SVG_OUT=ui.svg JC_BMP_OUT3D=part.bmp JC_BMP_OUTCAM=cam.bmp cargo test -p kmb-app --test svg_snapshot -- --ignored`。PowerShell では `$env:JC_SVG_OUT="ui.svg"; cargo test -p kmb-app --test svg_snapshot -- --ignored`。
- 実際の見た目をスクリーンショットで撮る(Linux のクラウドの作業場、GPU なし): `scripts/ui-shot/`。
  1. `cargo build -p kmb-app`
  2. `SHOT_OUT=<出力フォルダ> scripts/ui-shot/drive.sh scripts/ui-shot/screens.steps --sample`
  3. `design.png`・`sketch.png`・`cam.png` と、アプリの出力 `app.log` ができる。`JC_LOG=debug` を付けると、[5. ログ](#5-ログ) の内容も `app.log` に出る。
  - 暗い画面は `SHOT_THEME=dark`、起動を待つ秒は `WAIT`(既定 12)、実行ファイルは `APP` で替える。
  - 利用者の PC と同じ大きさと倍率で撮る: `SHOT_SIZE=3840x2068 SHOT_SCALE=2`(4K・表示倍率 200%)。決まった場面の試験(`apps/kmb-app/tests/screens.rs`)は `JC_SCREENS_SIZE=1920x1034 JC_SCREENS_SCALE=2`。
  - 手順のファイルは bash で、`shot 名前`・`click x y`・`rclick`・`move`・`key`・`type`・`wait` が使える(座標は 1280×760 の画面)。
  - 要るもの: Xvfb、xdotool、ImageMagick、mesa-vulkan-drivers(lavapipe)、libxkbcommon-x11-0、fonts-ipafont-gothic。描画は `WGPU_BACKEND=vulkan` と lavapipe(スクリプトの中で設定する)。
- 絵と動画: 材質の表示とレンダリングの作業空間の写真は `JC_SCREENS_OUT=out cargo test -p kmb-app --test screens write_render_screens -- --ignored`(96〜101。96 は「材質の質感」を切った前と同じ描き方)。削る様子の動画の見本は同じ所の `write_sim_video_sample`(102 の GIF)。人の手順で確かめる文書 `基準部品-ウォルナット.json` も同じ所に書く。CLI では `kmb export <文書> out.png`(静止画)・`out.gif`・`out.mp4`(回す動画)・`out.glb`。
  - glTF は Khronos の glTF-Validator(npm の `gltf-validator`)で誤り 0 を確かめる。MP4 は ffprobe と ffmpeg で読み、AV1 は絵に戻して元の絵と比べる(試験 `av1::tests::frames_become_a_readable_av1_mp4`。ffmpeg が無ければ中身は飛ばす)。
  - Windows の H.264(Media Foundation)は Windows でしか動かないので PC で: `cargo test -p kmb-io-render mf`(`--all` にも入る。Media Foundation の無い Windows では理由を出して飛ばす)。アプリでは回す動画を .mp4 で書き、知らせに「H.264」と出て再生できるかを見る。
  - 光線追跡と動画のコマは開発ビルドでも最適化する(ルートの Cargo.toml の `profile.dev.package.kmb-render`・`kmb-io-render`)。
- 人の操作の台本: `apps/kmb-app/tests/ui/*.jcs` を `cargo test -p kmb-app --test screens ui::` で流す(台本ごとに 1 つのテスト `ui::<台本の名前>` を build.rs が作り、nextest が並べて流す。`--all` にも入る)。1 本だけは `ui::熱の単位`、写真は `JC_SCREENS_OUT=out`(台本の `shot 名前` が `out/<台本>-<名前>.bmp`)。台本はコマンド欄と同じ英語の動詞で、画面の部品を名前で探して押す・打つ(`click "熱"`・`type "温度(℃)" 50`・`expect "温度(℃)" = 50`・`until "済み" 300s`)。名前はそのまま一致、`~字` で含む、`button:`・`field:` で種類、`#2` で 2 番目。欄はラベルの右か下の入力欄。1 行目の `#! sample part|cam|cam3d` で見本の文書から始める。1 行落ちると残りは飛ばし、落ちた行と前後のログを出す。押す・打つ・確かめる行は、再計算とホームのフォルダの読み直し(`HomePage::loading`)が終わるまで待ってから流れる(人も結果と一覧が出てから次を押す。`until` だけは待たずに見続ける)。
- 部品を引く: `drag <面の名前> <右へ> <下へ>` は 3D のビューでその面の見えている所を左で押し、画面の点で右・下へ 8 コマに分けて引いて離す(押したまま止まると長押しの重なりの一覧になるので、押した次のコマから引く)。`円柱のジョイントを引く.jcs` がこの形。
- 辺を押すのと、ホイールで巻くの: `pick f12:end|f12:side:e3` は両側の面の名前を `|` でつないで、その辺の見えている所を押す。`scroll <部品> <点>` は部品の上にマウスを置いてホイールで下へ巻く(G-code の窓は見える行しか部品にならないので、`scroll ~"(kieMonoBench / Carvera Air)" 1000000` で終わりまで巻いてから `expect ~"G3 …"` で確かめる)。`ねじ切り.jcs`・`トレース.jcs` がこの形。
- ツールバーの畳み方: 台本の 2 行目の `#! size 1280x800` で画面の論理の大きさを決める(`JC_SCREENS_SIZE` があればそちらが勝つので、PC と同じ 4K・200 % で同じ台本を流せる)。`ツールバーを画面に収める.jcs` が幅 1280 で設計・加工・スケッチの右端まで押せることと、ボタンの名前を出さない設定を確かめる。字とアイコン 100 %・250 %、幅 1920・1280、名前の有無の写真は `JC_SCREENS_OUT=out cargo test -p kmb-app --test screens write_toolbar_screens -- --ignored`。
  - 書き出しの台本: 先頭に `#! files` を書くと、保存の窓は人が「保存」を押したのと同じく勧めた名前で一時フォルダ(`<temp>/kmb-ui-scripts/<台本>/`、書き出した順の番号つき)に書く。書いたファイルの中身は `screens.rs` の `check_script_files` に台本の名前ごとに確かめを書く(例 `3MFとSVGの書き出し.jcs`: 3MF の単位と色、SVG の mm の大きさ)。
  - 中の値を直接書き換える試験は画面の不具合(欄の単位など)を見逃す。新しい欄や窓を作った時は、人の手順の台本を 1 本足す。台本の中の名前が分からない時は `tree` で画面の部品の一覧を出す。
  - 動いているアプリでも同じことができる: MCP の `app` の `ui_tree`・`ui_click`・`ui_type`・`script`、またはアプリの 表示 → コマンド の欄に打つ。
  - 入力欄はみな既定の単位か例外の印(`kmb_core::FieldUnit`)を持つ。持たない欄は `units_tests::every_number_field_declares_its_default_unit_or_an_exception` が名前を挙げて落ちる。
- 写真と台本で部品を押す時は、名前の完全一致で探し、押せなければ止める(部分一致は後から足した似た名前の部品に当たる。▾ の付くボタンの名前は ▾ を除いた字)。
- 画面を変えた時は、撮った画像を前の版や試作と並べて差を確かめ、報告に添える(テストが通るだけで終わりにしない)。

## 5. ログ

### 置き場所

| OS | フォルダ |
| --- | --- |
| Windows | `%APPDATA%\kie-monobench\logs\` |
| Linux | `~/.local/share/kie-monobench/logs/`(`XDG_DATA_HOME` があればその下) |

- 画面(`kie-monobench`)・CLI(`kmb`。ログの名前は `kmb-cli`)・MCP(`kmb-mcp`)の起動ごとに 1 ファイル(`<アプリ>-<UTC の日時>-<pid>.log`)。どこへも送らない。
- アプリごとに新しい 20 ファイルを残す(CLI を何度呼んでも画面のログは消えない)。1 ファイルが 20 MB を超えたら、それ以降は書かない。
- 1 行ずつ書くので、SMLib の C++ の中で落ちても直前の行まで残る。
- 時刻は UTC(日本時間は 9 時間足す)。行の形: `2026-10-14T12:34:56.789Z INFO  [スレッド名] kmb_core::document: 変更: 押し出し`
- クラッシュ報告(`crash` フォルダ)には、そのログのファイル名と終わりの 200 行が入る。パニックでない異常終了でも、次の起動で前回のログの終わりを報告に添える。

### 何が書かれるか(既定の `info`)

| 出どころ | 内容 |
| --- | --- |
| 起動 | 版、OS、pid、ログのファイル、引数、形状カーネル(SMLib か truck) |
| 文書 | 開いた・保存した・自動保存した・復旧した(失敗は理由つきで `error`)、設定の読み込み |
| 変更 | 変更・取り消し・やり直しの名前 |
| 再計算 | 機能の数、評価した数、再利用した数、誤りの数、かかった時間。機能の誤りは `warn` で機能の名前と理由 |
| 操作 | 実行した操作(G-code の書き出しなど)の結果の要約と時間。時間のかかる仕事(シミュレーション)は始めと終わりと時間。失敗は `warn` |
| パニック | 理由と場所(`error`) |

`debug` にすると、機能ごとの評価時間と、操作の結果の中身も書く。

### どこまで書くかを変える(`JC_LOG`)

| 書き方 | 意味 |
| --- | --- |
| (設定しない) | 自前のモジュールは `info`、ほかのライブラリ(wgpu など)は `warn` |
| `debug` | 自前のモジュールを `debug` まで |
| `kmb_cam_commands=trace` | その target(モジュールのパス)の始まりが一致するものだけ変える |
| `*=info` | ほかのライブラリのレベル |
| `info,kmb_core::recompute=debug,*=warn` | コンマで並べる。長く一致した方を使う |

`JC_LOG` を設定した時は、同じ内容を標準エラーにも出す。PowerShell の例:

```text
$env:JC_LOG="debug"; .\target\release\kie-monobench.exe
Remove-Item Env:JC_LOG
```

Windows のリリース版は窓のアプリなので(起動の時に黒い窓を出さない)、端末から起動しても標準エラーは端末に出ず、
PowerShell は終わりを待たずに次の入力へ戻る。ログのファイルにはいつも通り書く。端末で見たい時は、パイプで受けると
終わるまで待って出す(デバッグ版は今まで通り端末に出す):

```text
$env:JC_LOG="debug"; .\target\release\kie-monobench.exe 2>&1 | Out-Host
```

Windows では、本体の窓を最初の画面を出すまで DWM で覆う(apps/kmb-app/src/win_start.rs。最大化して閉じた後の起動では、
eframe が覚えた最大化で窓を作り、winit が隠したままの最大化の窓を設定の変更のたびに表示していた)。ログの `info` に、
eframe が窓に渡す値(「窓の作り方: 位置…、中の大きさ…、最大化…」)、起動元からの窓の出し方
(「起動元からの窓の出し方: dwFlags=…」)、「起動の窓を覆った(どの時点か…)」「起動の窓の覆いを外した」と、描く前に来た
最大化・戻すの要求(「起動の窓に SW_MAXIMIZE(3)の要求」と呼び出し元)を書く。窓の出入りを外から確かめるには、
最上位の窓の 作った・出た・消えた・動いた を記録する見張り(Win32 の窓のイベントを外から拾う小さなスクリプト。リポジトリの外に置く)を使う。

Claude デスクトップ(MSIX)の中のセッション(Remote Control など)から起動した JC は、%APPDATA% の読み書きが
`%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\kie-monobench\` の写しへ振り向けられる。
利用者の窓の状態(data\app.ron)・設定・crash・最近使ったファイルを使う確認は、利用者の起動か、パッケージの外からの起動で行う。
本物のファイルをそのセッションから読むときは `\\localhost\C$\Users\…` の道を使う(起動時の窓の調べで、写しを読んで取り違えた)。

起動の区切りごとの時間は、いつも `info` で「起動の区切り: …(この区切り … ms、開始から … ms)」と書く
(ログを始める → 設定を確かめ文書を読む → 作業の場を組み立てる → 窓と描画装置を用意する → 3D のビューを用意する
→ 日本語のフォントを読む → 最初の画面を作り始める → 最初の画面を描き終える → 開いた文書の再計算が終わる)。
3D のビューのシェーダーと管は別のスレッドで作るので、「3D のビューを用意する」は始めるだけの時間になる。
作るのにかかった時間は「3D のビューを GPU で描く: …(シェーダーと管を作るのに … ms)」、最初の画面で
作り終えるのを待った時間は「3D のビューの用意を待った: … ms」に出る。

### モジュールにログを足す時の決まり

各 crate は `log` のマクロ(`log::warn!` など)を使う。依存は `log.workspace = true`。書き出しの仕組み(`kmb_session::logging`)には依存しない。

| レベル | 使う時 |
| --- | --- |
| `error` | 結果を出せなかった(保存できない、操作が失敗した、評価中のパニック) |
| `warn` | 結果は出たが、知るべきことがある(機能の評価の誤り、読めない設定、古い形式の変換) |
| `info` | 1 回の操作に 1〜数行(開いた、保存した、実行した、要約と時間) |
| `debug` | 調べる時だけ見る中身(機能ごとの時間、経路の点の数、引数) |
| `trace` | ループの中など大量に出るもの |

- 毎フレーム呼ばれる所(画面の描画)では `info` 以上を出さない。
- 文書の中身や大きなデータを丸ごと書かない(大きさや数を書く)。個人のファイルのパスは書いてよいが、それ以外の個人情報は書かない。
- 変更・再計算・操作の実行はコア(`kmb-core`)がまとめて書くので、各モジュールで同じことを書き直さない。モジュールでは、そのモジュールの中でしか分からないこと(戦略が工具の径を変えた、カーネルが近似に切り替えた等)を書く。

## 6. PC で時間がかかる所と対策

### 入れた対策

1. **SMLib の C++ の結果を取っておく**(`crates/smlib-sys/build.rs`)。ソースと見出しの中身・コンパイラの場所と引数と環境・build.rs 自身から指紋を作り、`target/smlib-cache/<指紋>/` に静的ライブラリを置く。同じ指紋ならコンパイルせずに写す。
   - clippy・テスト・リリースビルド・Rust の版の切り替え・rust-analyzer のどれでビルドスクリプトが走り直しても、C++ は 1 回だけ。クラウドで clippy の段が 3 分 31 秒から 12 秒になった(smlib-sys のビルドスクリプトは 169 秒から 0.2 秒)。
   - git の取り出しで時刻だけ変わっても、中身が同じなら作り直さない。
   - 新しい方から 4 つまで、合わせて 512 MB までを残す。消したい時は `cargo clean` か `target/smlib-cache` を消す。使わない時は `SMLIB_CACHE_DIR=off`。
   - PC では取っておいた `smlib.lib` が約 2 GB あった(Linux は 39 MB)。上流の見出しが Windows では DLL を作る前提で、全クラスに `__declspec(dllexport)` を付け、多くのテンプレートを見出しの中で明示的に実体化するため、どの .cpp にも全クラスの関数が書き出されていた(1 ファイル約 9 MB)。静的にリンクするだけなので、ビルドスクリプトが該当する見出しを `OUT_DIR/static-inc` に書き直して先に探させ、Linux と同じ形にした(`write_static_headers`)。Windows 向けの形をクラウドの clang で試すと 1 ファイル 4.5 MB → 0.4 KB、`SmBrep.cpp` 6.1 MB → 1.1 MB、全 234 ファイルで 21 MB(確定度 A、PC の MSVC では未測定)。中継層(`kmb-kernel-smlib`)も同じ見出しを使う。
2. **truck の回は、カーネルを選び替える crate だけ**(1 節)。
3. **truck を包む crate(`kmb-kernel-truck`)を開発ビルドでも最適化**(根の `Cargo.toml`)。truck での重い試験が約 3 割速くなった(`kmb-ui` 287 秒 → 200 秒、`kmb-samples` 302 秒 → 249 秒、`kmb-solid` 98 秒 → 67 秒)。
   - 2・3 を入れた後、クラウドの `cargo xtask verify --all --truck` は 15 分 40 秒から 12 分 22 秒になった(truck の回 769 秒 → 575 秒。通過 525、失敗 0、無視 4)。
4. **PC での取り込みの検査は `cargo xtask verify --dependents`**(truck の回なし)。全体はクラウドで手渡す前に `--all` で済ませている(truck の回は 1 節の決まりで足す)。
   - 失うもの: (a) 取り込みが触らない crate の Windows での再試験(コードも依存も変わっていないので、結果は前回と同じになるはず)、(b) truck のカーネルでの Windows の試験(truck は Rust だけで書かれ、Windows と Linux で同じコードが動く)。SMLib(既定のカーネル、C++)での Windows の試験と clippy は、取り込みが触る範囲で今まで通り行う。
   - 根の `Cargo.toml`・`Cargo.lock`・`.cargo/` を変えた取り込みは、自動で全体を検査する(1 節)。
   - truck の回は公開の準備の時だけ流す(1 節)。

### 今の対策のほか

- nextest でテストの実行ファイルをまたいで並べ、SMLib の回・truck の回・clippy を同時に流す(1 節)。truck の回は形の計算の試験だけ(1 節の `TRUCK_TESTS`。同じ試験が truck では SMLib の 3.5〜15 倍かかり、加工そのものはどちらのカーネルでも同じ処理なので)。
- 工具の形(`kmb-cam-tool`)・加工領域(`kmb-cam-geom`)・経路(`kmb-toolpath`・`kmb-toolpath-moves`)の crate は開発ビルドでも最適化する(根の `Cargo.toml`)。O2 の crate から呼ばれても、最適化なしの crate の関数はインライン化されず遅いため。
- クラウドでは SMLib の C++ の結果をコンテナをまたいで使い回す(`/mnt/project-files/.cache/smlib-sys`。`.cargo/config.toml` の `SMLIB_SHARED_CACHE`)。
- 時間を測る試験は、決めた上限ではなく実際にかかった時間と比べる(混んだ機械で落ちないように)。
- 効かなかったので入れていない: SMLib の C++ を先に作り始める(初回の組み立ては 4 コアとも埋まっていて縮まない)。

### 試していないこと(効きそうだが、PC での確認が要る)

- **Windows のリンカを rust-lld に替える**: `link.exe` より速いことが多い(テストの実行ファイル 10 本ほどが 50〜70 MB、PDB が 1 本 280 MB ある)。`.cargo/config.toml` に `[target.x86_64-pc-windows-msvc] linker = "rust-lld.exe"` を足すだけだが、SMLib の MSVC のライブラリと組めるかと速さを PC で測ってから決める(確定度 U)。
- **Windows Defender のリアルタイム保護から target フォルダを外す、Dev Drive に置く**: Windows でのビルドが速くなることが多いが、PC の設定を変えるので利用者が決める。

## 7. 画面の速さを測る・描き方を選ぶ

### 速さを測る(`JC_BENCH_VIEW`)

`JC_BENCH_VIEW=<フレーム数>` を付けて見本を開くと、等角で回す・上から見て左右へ動かすを、それぞれその数だけ続けて(垂直同期なし)、フレームの間隔をログと標準エラーに書いて閉じる(Windows のリリース版で端末に出すには、下の行の終わりに `2>&1 | Out-Host` を付ける)。

```text
$env:JC_BENCH_VIEW="300"; .\target\release\kie-monobench.exe --sample
Remove-Item Env:JC_BENCH_VIEW
```

出力の例: `画面の速さ(等角で回す、299 フレーム): 平均 … fps(1 フレーム … ms、中央値 … ms、遅い方から 5% … ms)`。CPU の描画と比べる時は `JC_VIEW3D=cpu` も付ける。

`JC_LOG=trace` なら、毎フレームの画面作りの時間(`1 フレームの画面作り`)と 3D のビューを描いた時間(`3D のビュー(GPU)`)も出る。

### 3D のビューの描き方

| 場合 | 描き方 |
| --- | --- |
| 既定(GPU の描画装置) | GPU(`kmb-ui-view3d` の `gpu3d`。MSAA 4 倍) |
| ソフトウェアの描画装置(WARP・llvmpipe)、安全モード、`JC_VIEW3D=cpu` | CPU(`view3d::render_guided`。前からの描き方) |
| `JC_VIEW3D=gpu` | ソフトウェアの描画装置でも GPU の描き方(クラウドの画面の点検用) |

画面の試験(egui_kittest、`screens.rs`)は GPU を使わないので、CPU の描き方で撮る。GPU の描き方の見た目は、`JC_VIEW3D=gpu scripts/ui-shot/session.sh 1 start --sample` で撮って CPU の描き方と並べて確かめる。

### 画面の速さの表示と、拾う速さ

- 設定の「3D の表示」→「画面の速さを出す」を入れると、3D のビューの右下(「注意」の札の上)に `… fps ・ 1 回の処理 … ms(描く …・拾う …)` が出る。直近 1 秒の平均。画面は動かしている間だけ描き直すので、fps は回したり動かしたりしている間の値を見る。
- `JC_BENCH_VIEW` の出力にも「うち 3D を描く … ms、拾う … ms」が付く。部品の再計算が終わるまでは数え始めない。
- 拾う速さだけを前の拾い方(三角形を全部写す)と比べる: `cargo test -p kmb-ui --release --lib pick_speed -- --ignored --nocapture`。
- 重い部品(200×200×10 の板に 20×20 の穴)を作る: `JC_HEAVY_OUT=heavy.json cargo test -p kmb-ui --release --lib write_heavy_part -- --ignored`。
- 面は網ごとの箱の木(`pick_bvh`)で拾う。三角形 2 万以上の網は裏のスレッドで作り、できるまでは前の拾い方。辺は GPU があれば番号の絵(`gpu3d::edge_at`)で拾い、無ければ CPU。GPU と CPU の答えが合うかは `gpu_edge_ids_pick_the_same_edges_as_the_cpu`(GPU が無い所では飛ばす。クラウドは `mesa-vulkan-drivers` を入れると lavapipe で通る)。

### 描画の API と DXC

設定の「描画」→「描画の API」(次の起動から)で、Windows の描画の API を選ぶ。

| 選択 | 中身 |
| --- | --- |
| DirectX 12(DXC)(既定) | DirectX 12 だけを探す。シェーダーを DXC(実行ファイルの横の `dxcompiler.dll`)で作る。DLL が無ければ FXC で作り、ログに出す |
| DirectX 12(FXC) | DirectX 12 だけを探す。シェーダーを Windows に入っている FXC(`d3dcompiler_47.dll`)で作る |
| Vulkan | Vulkan だけを探す |

- 前は wgpu の既定(`Dx12Compiler::Auto`: 静的に組み込んだ DXC → `dxcompiler.dll` → FXC の順に探す)で、DXC を組み込まず DLL も置いていなかったので、FXC になっていた(PATH に `dxcompiler.dll` がある PC を除く)。探す API も Vulkan・DirectX 12・GL の全部だった。
- DXC を置く: `cargo xtask dxc`。Microsoft の公式の配布(GitHub の microsoft/DirectXShaderCompiler、v1.9.2609)を `curl` で取り、SHA-256 を確かめ、`target/debug` と `target/release` に `dxcompiler.dll` と許諾の文(`dxcompiler-LICENSE-LLVM.txt`)を置く。取ったものは `target/dxc/` に残る。`cargo xtask sweep` は消さない。配る時は exe と同じフォルダに、この 2 つを入れる。
- DirectX 12 Ultimate(機能の段 12_2)は別の API ではない。wgpu は GPU が対応するいちばん高い段を自動で使う。上位の機能(メッシュシェーダー・レイトレーシング)はシェーダーモデル 6.5 以上が要り、FXC(5.1 まで)では使えない。使えるかは、設定の「今:」の行とログ(`描画の API:`)に出る。
- 調べる時: `JC_RENDERER=dxc|fxc|vulkan|auto` で設定に代えて選べる。wgpu の環境変数 `WGPU_BACKEND`・`WGPU_DX12_COMPILER` があれば、そちらが勝つ。

### 起動の安全策

| 起きたこと | どうなるか |
| --- | --- |
| 設定ファイル(`settings.json`・`recent.json`・`projects.json`・`layout.json`)が読めない | `<名前>.bad-<時刻>` へ名前を変えて退け、既定で始める。知らせに出る |
| 窓が開く前に 2 回続けて止まった(`startup.json` で数える) | 次は安全モードで始める |
| 設定で選んだ描画の API(既定は DirectX 12(DXC))で窓を開けない | wgpu の既定の選び方(`JC_RENDERER=auto`)で開き直し、知らせに出す |
| GPU で窓を開けない | ソフトウェア描画(`JC_RENDERER=cpu`)で開き直す。それもだめなら理由とログの場所を窓で知らせる |
| `--safe-mode` か `JC_SAFE_MODE=1` | 安全モード: 既定の設定と画面の配置、ソフトウェア描画、自動保存の復旧を出さない。設定は書かない(次の普通の起動で元に戻る)。窓の題に「安全モード」と出る |

## 8. target を膨らませない(`cargo xtask sweep`)

cargo は、依存の機能・プロファイル(`Cargo.toml` の `[profile]`)・Rust の版・ビルドスクリプトが変わるたびに、別の名前(`<crate>-<16 桁の指紋>`)で作り直し、古いものを消さない。PC の target は 2026-10-17 に約 101 GB あった(debug の deps 37 GB・incremental 27 GB・build 9 GB、release 27 GB)。

`cargo xtask sweep` は、今使う組み立てを cargo に問い合わせ(`--message-format json`)、そこに出てくる指紋だけを残して、ほかを消す。

| 何を | どう選ぶか |
| --- | --- |
| `deps`・`build`・`.fingerprint`・`examples` | 名前の指紋が、今の組み立てに無いもの |
| `incremental`(自前の crate のコンパイルの途中結果) | 名前から指紋が分からないので、`--keep-days`(既定 3)日より前から使っていないもの |

- 問い合わせる組み立て: テスト(`cargo test --workspace --no-run`)、clippy(`--workspace --all-targets -- -D warnings`)、xtask。`target/release` にアプリ(`kie-monobench`・`kmb`・`kmb-mcp`)があれば、その `cargo build --release -p ...` も(`cargo xtask release` と同じパスの置き換えの引数を付ける。12 節)。足りないものがあれば、問い合わせの中で作る(次のビルドで作るはずだったもの)。
- 既定は見積もりだけ(分類ごとの GB を出し、何も消さない)。`--yes` で消し、消した後にもう一度問い合わせて、作り直した crate の数を出す(0 なら次のビルドは遅くならない)。
- 次のビルドへの影響: deps・build・.fingerprint は今の組み立てが使わないものだけなので、無い。incremental を消した crate は、次にその crate を直した時に 1 回だけ全部をコンパイルし直す。rust-analyzer の `cargo check` の結果は問い合わせに入らないので消え、次に開いた時に自前の crate を調べ直す。
- SMLib の C++ の取っておき(`target/smlib-cache`)は build.rs が新しい 4 つ・512 MB までに保つので、対象にしない。
- 取り込みで `Cargo.toml` の `[profile]` や依存が変わった後は古いものが一度に増えるので、その後の検査が通ってから動かすとよい。

### 最小限の target(PC で一から作り直して測った)

`cargo clean` で全部消し、`cargo xtask verify --all` と `cargo build --release -p kmb-app` を流した後の大きさ。これが今の版で残すべき最小の状態(2026-10-17 の PC で約 11 GB)で、これより大きければ古い版の積み上がり(`cargo xtask sweep --yes` で消せる)。

- 残すもの: debug の deps・build・incremental と、release の deps・build・アプリの exe。`nextest`・`tmp`・`verify` は検査の時だけ使い、作り直しの直後は空。
- 依存の機能が組み立ての選び方で変わると、同じ版でも 1 組(クラウドで約 7 GB)増える。`--all` は `--workspace` で xtask も入り、`-p` で選ぶ時は入らない。xtask は hakari の対象から外しているので、xtask だけの依存の機能が `--all` の時だけ全体に混ざる。2026-10-22 は xtask の `sha2`(既定の `std`)が `sha2`・`digest`・`crypto-common` を std にして wasmtime から、`toml`(既定の機能)が `toml` を std・display にして egui_kittest(画面の試験)から、アプリと試験まで全部の指紋が変わり、`--all` の後の `--dependents` で 14.3 GB → 22.2 GB になった。xtask の `sha2` と `toml` の機能を、ほかの crate が使う分だけに絞って揃えた。`cargo tree -e features` は試験だけの依存(dev)の機能の合わさり方を正しく出さないので、確かめは `target/debug/.fingerprint/<crate>-<指紋>/lib-*.json` の `features` を比べる。
- 検査(`verify`)は組み立てが通った後、空きが `JC_SWEEP_BELOW_GB`(既定 12)GB を切っていれば `sweep --yes` を流す。cargo は依存が変わるたびに古い組を残すので、クラウドの作業場(枠は約 39 GB、使えるのは約 30 GB)では main を 2〜3 回取り込むと溢れていた(空から 14.3 GB → 依存の変わった main で 29.2 GB、空き 0.3 GB)。
- 検査(`verify`)は組み立ての前にも 2 つ片付ける。(1) 揃えた依存(`crates/kmb-workspace-hack/Cargo.toml`)が前の組み立てから変わっていれば `target/debug/incremental` を消す。自前の crate は全部この crate に依存するので、ここが変わると全部の指紋が変わり、古い incremental は 1 つも読まれない。(2) 空きが `JC_SWEEP_BELOW_GB` を切っていれば、incremental の前の回を消す。rustc は crate ごとにいちばん新しい回と 1 つ前の回を残すが、次に読むのは新しい回だけ。`sweep` もこの前の回を消す。
  - クラウドの検査だけ incremental を切る案(`CARGO_INCREMENTAL=0`)は入れない。incremental は 4〜5 GB で済むようになったうえ、切ると kmb-ui の 1 行の変更の組み立てが 7 秒 → 31 秒、戻した直後の `--all` の組み立てが 70 秒 → 約 310 秒になる(2026-10-23 にクラウドで測った)。同じ日の再発の時に測り直しても、`cargo test -p kmb-ui --no-run` で 6 秒 → 22 秒(切り替えた 1 回目は 104 秒)だった。
- 依存の宣言が変わった時。揃えた依存のほかに、どれかの crate の `Cargo.toml` の依存が変わっても、その crate から下流の指紋が全部変わり、古い組の隣に新しい組ができる(cargo の指紋は依存先の指紋を含む)。kmb-interchange の依存が変わった main に、その前の target を戻して `verify kmb-session --dependents` を流すと、組み立ての 2 分で deps 12 → 17 GB・incremental 5.5 → 8.2 GB、空き 12.6 → 4.0 GB になった。新しく作ったテストの実行ファイル 17 個(5.1 GB)すべてに、指紋の違う古い同名の物が残っていた。検査の後の sweep は、cargo への問い合わせ(`cargo test --workspace --no-run`)で選ばなかった crate のテストまで作りに行き、空きの少ない所でさらに場所を使っていた。戻した組は #250 より前の `save` が置いた物で、戻す大きさを書いていなかったので、`restore` の空きの確かめも働かなかった。直し(`xtask/src/space.rs`):
  - 組み立ての間、空きが `JC_WATCH_BELOW_GB`(既定 6)GB を切れば、incremental の前の回と、この組み立てで同じ名前の新しい物ができたテストの実行ファイルを消す(実行ファイルはほかのコンパイルに読まれないので、途中で消しても壊れない)。何か消したら、消し過ぎた物を作り直すために組み立てをもう一度流す。
  - 検査の後の片付けは、空きが `JC_SWEEP_BELOW_GB` を切っていれば、まずこの検査で作り直して要らなくなった物(古い同名のテスト・自前の lib、同じ crate の使われなかった incremental のフォルダ、incremental の前の回)を消す。cargo に問い合わせる sweep は `--all` が通った後で、それでも足りない時だけ流す。直す前の再現の後の target で見積もると、前者で 9.5 GB 空く。
  - 採らなかった案: テストと実行ファイルのデバッグ情報(行番号だけ)を zlib で縮める(リンクの `-Wl,--compress-debug-sections=zlib`)。kmb-ui のテストの 404 MB のうち 256 MB がデバッグ情報で、縮めると 200 MB になり、panic の「ファイル:行」も出る(zstd だと出ない)。ただ 430 MB の 1 本を縮めるのに約 8 秒かかり(写すだけなら 2 秒)、1 行の変更で数本を作り直すたびに遅くなる。上の直しで空きは足りるので入れない。`RUSTFLAGS` で渡すと全部の指紋が変わる(linker を替える形なら指紋は変わらず、1 回だけ全部を作り直す)。
- 101 GB まで膨らんだのは、古い版の deps・build・incremental の積み上がりだった。取り込みの後などに `cargo xtask sweep --yes` を流せば、上の最小の状態(約 11 GB)に近く保てる。
- 自前の crate のデバッグ情報は行番号だけにする(根の `Cargo.toml` の `[profile.dev]`。panic の backtrace に関数名とファイル名・行は出る)。incremental はやめない(1 ファイルを直した後の組み立てが約 2 倍遅くなるため)。

## 9. 計算の速さ

G-code(`kmb gcode`)とシミュレーションの報告(`kmb sim`)を変えずに、再計算・面取り・フィレット・仕上げ・シミュレーションを速くした時の決まり。

- 加工の操作とセットアップは、互いに依存しなければ別々のスレッドで計算する(kmb-core の `CONCURRENT` の印。結果は機能の順に並べる)。数は既定で CPU の数、`JC_RECOMPUTE_THREADS=1` で 1 つずつ(比べる時)。ログは `JC_LOG=info,kmb_core::recompute=debug` で「同時に計算した: …」。
- 面取り・フィレット・穴は、ブーリアンの前後の体積を比べて壊れた形を見つける。前は表面積と外形の箱まで求める `mass` を毎回 2 回呼んでいた(この時間が 9 割)。体積だけの `Kernel::volume` を使い、SMLib は形ごとに覚える。
- 工具を下ろす計算(kmb-cam-dropcutter)とシミュレーション(kmb-sim)は、格子の行を 8 セルの塊に分けた「塊のいちばん高い所」で、先端を上げられない(削れない)塊を飛ばす。最大を取る・高さを下げるだけの計算なので、結果は全部のセルを見た時と同じ(試験で比べる)。
- シミュレーションを帯に分けて複数スレッドで削る案(P3)は、食い込み・早送りの切削の件数の上限や、削ったかどうかで次の調べをするかが決まるので、帯ごとに分けると結果が変わる。上の飛ばし方の方が、結果を変えずに速い。
- リリース版の LTO(`lto = "thin"`、`codegen-units = 1`)は入れない。速さは誤差の内(再計算 2.25〜2.39 → 2.26〜2.31 秒、シミュレーション 1.68〜1.77 → 1.61〜1.74 秒。時間の多くは SMLib の C++ で、Rust の LTO が効かない)。1 ファイルを直した後の組み立てが 13 → 113 秒、一から 6 分 52 秒 → 7 分 7 秒になる。
- 測り方: `valgrind --tool=callgrind target/release/jc recompute <file>` → `callgrind_annotate --inclusive=yes`。リリース版に行番号が無くても関数名で足りる。

## 10. 解析の確かめ方(kmb-fea)

- `cargo xtask verify kmb-fea` で、教科書の答えのある形と照らす(`crates/kmb-fea/src/validation_tests.rs`)。網は形に沿った 2 次の四面体(`tetmesh.rs`)。2026-10-20 の値:
  - 箱(100×10×10、2.5 mm): 網の体積と 6 面の面積が一致。片持ち梁の先のたわみ 0.76% 差、真ん中の上の面の曲げ応力 0.04%、棒の引張(伸び・応力が一致)、1 次元の熱伝導(どの節点も 1e-5 ℃ 以内)、ピン・フィンの先の温度 0.01%、1 次の固有振動数 0.28%。塗る値(Saint-Venant の解との差)は横の面で RMS 0.055%・最大 0.22%。
  - 曲面: 厚肉の円筒の内圧(Lamé、平面ひずみ)で内の面の変位 0.02%・内の面の周の応力 0.63%・外の面 0.14%(曲面の上の辺の中点は、両端の角の値を中点の向きへ回してから平均する。そのまま平均すると半径の向きの応力が混ざり、中点が −1.2% と出て全体で 1.02% だった。残る −0.6% は角の値で、要素の中の応力が直線なので面の近くの急な変化を表せない分。h² で減る: h 0.6 で −0.3%)。塗る値は内の面で平均 −1.11% → −0.64%、ばらつき 0.42% → 0.25%(`recovery_lab/known.rs` の `lame_inner_hoop_parts`)。中空の円筒の熱伝導(対数の分布)で最大 0.05 ℃(100 ℃ の差のうち)。丸棒の片持ち梁の 1 次の固有振動数 0.16%。
  - 誤差の分布と縁のギザギザ(`recovery_lab/` の ignore の試験。`known.rs` の `lame_inner_hoop_parts`、`clamped.rs` の `top_face_error_map`・`clamped_edge_profile`・`clamped_edge_nodes`。`JC_LAB_DIR=dir cargo test -p kmb-fea --lib top_face_error_map -- --ignored`): 箱 60×40×10 の一端を固定し上の面を押す。細かい規則正しい網(h 1)の解と比べ、h 2 の上の面の誤差は固定した縁から 5〜30 mm で RMS 0.07%・最大 0.63%、1〜5 mm で RMS 3.4%(特異点のまわりの 1 要素ぶん)。固定した縁に沿う値の上下(4 mm の移動平均からの外れ、辺の中点は角の平均)は縁の上 0.82%、縁から 1〜2 mm で 2.5%。前の格子の網は軸にそろった箱でだけ縁に沿って規則正しく並ぶため、この箱では 0.09%・1.7% と小さい。
  - 鋭い縁に沿う帯(`surfmesh/band.rs`・`volmesh.rs` の `band_points`): 縁の値のむらは点の位置ではなく縁に沿う網のつながりで決まる(格子の点を ±25% 崩してもつながりが同じなら 1 mm で 0.39%、形に沿う網は 2.57%)。そこで鋭い出っ張った縁の両側の面に縁の点ごとの列を置き、縁と列の間を同じ向きの対角で分け、中には平行四辺形の 4 つ目の点を足す。結果(h 2、`a_clamped_edge_is_smooth`・`a_clamped_round_edge_is_smooth` で確かめる): 箱の固定した縁から 0.5 mm で 1.29% → 0.22%、1 mm で 2.57% → 0.36%、2 mm で 2.5% → 0.47%。円柱(半径 10、96 角形)の根元の輪で、1 mm 2.98% → 0.67%、2 mm 2.82% → 1.14%(周に沿って滑らかな式に合わせた残り)。要素の数はほぼ同じ(箱 41411 → 約 41000、円柱 21938 → 21878)。
    - 作りの注意: 帯の四角と縁の点ごとの断面が長方形だと 4 点が同じ円に乗り、中の網の Delaunay が対角を決められず帯が崩れる(取り戻しで帯を割り、むらが 2.9% に戻った)。また断面の 4 点から潰れた四面体ができ、細分が同じ所に点を足し続けて点の上限を超えた。列の点を縁に沿って縁の点の間の 5% だけ戻して防ぐ(どの点も)。細分は潰れた四面体の中心(角の点と重なる)には点を足さない。
    - 曲がった縁は曲がりが 90° ごとに帯を分け、分けた所の縁の点と列の点は両方の帯で使う(1 点あけると、あけた所の値が上下した)。帯の三角形は重心が弦のたるみの 8/9 まで面から離れる(帯の無い網は 2/3)ので、帯を置く曲がった縁は点の間を √(3/4) 倍に細かくする。
    - 帯のまわりの埋め直しは縁に沿う座標の上で耳を切り、多角形の中の Delaunay へ入れ替える(細長い所で 1 点からの扇ができ、曲面では面を突き抜けた)。除いた三角形と置いた三角形の面積が 2% より違えば置かない。
    - まだ置かない所: へこんだ縁(入隅)、面の間の角が 30° 未満か 150° を超える縁、ほかの縁から列の幅の 2 倍以内(角の近く)。表示用の網の 96 角形のような面の折れ目は、そのまま形として残る(折れ目の数を 384 にすると円柱の根元 1 mm で 0.74%)。
  - 軽い形の参考で、体積(要素の体積で重みを付けた割合)が決めた割合になり、コンプライアンスが下がること。
  - 網そのもの(`tetmesh.rs` の試験): 箱は体積が一致し鋭い辺が全部残る、円柱・中空の円柱は表示用の網の体積と 0.2% 以内、形から網へのずれが 0.1 mm 未満。
- 通しの確かめは kmb-samples の `fea_tests.rs`(スケッチ → 押し出し → 研究を足す → 長さを変えて解き直す → 表示の切り替え、見本の部品で網の確かさの数値が出ること)。
- 結果の数値に、網の確かさ(体積の差・表面のずれ・形の良い要素の割合と二面角、鋭い縁に沿う帯を置いた縁の点の数)を出す。要素の大きさより小さな穴などは粗くなり(見本の部品の直径 3.4 mm の穴は 2 mm の網で形から網へ最大 0.84 mm)、その時は結果の注意に出す。
- 特異点の候補(`singular.rs`): 固定した面の縁と入隅に印を付け、印から要素 2 つ分の内を「近く」として「特異点を除いた最大」から除く。ただし、印の辺とつながらない別の鋭い辺と、その辺に接して印に接しない面(穴の内側など)の方が近い節点は除かない。見本の部品の板の端を固定し要素の大きさ 2.5 mm で解くと、固定した面から約 2 mm の穴の縁がいちばん高い(8.68 MPa)。前はその穴の縁が近くに入り、「特異点を除いた最大 5.42 MPa」と特異点の注意が出て強さを甘く見ていた。試験 `a_hole_next_to_the_clamped_face_is_not_taken_as_singular`、e2e `tests/ui/特異点の近くの穴.jcs`(結果の数値にその行と注意が出ないこと)
- 画面: `JC_SCREENS_OUT=out cargo test -p kmb-app --test screens write_tet_mesh_screens -- --ignored`(120〜123: 表面・断面で切る・網の質・結果の数値)。人の手順で確かめる文書 `見本の部品-静的応力.json` も同じ所に書く。
- ログ: `JC_LOG=info` で、網の数値(`解析の網: MeshStats {…}`)、代数的マルチグリッドの段の数、共役勾配法の回数、研究ごとの要素の数・解いた時間を出す。

## 11. 流れと空力の確かめ方(kmb-flow)

- `cargo xtask verify kmb-flow` で、答えのある形と照らす(`crates/kmb-flow/src/vlm.rs` と `src/validation_tests.rs`)。楕円翼の揚力傾斜(Helmbold の式と 3% 以内)と翼の効率 e(1 から 4% 以内)、長方形の翼の中立点(前縁から翼弦の 0.2〜0.28)、平行な板の間の流れ(放物線と 1% 以内)、四角い管の流れ(級数の解と 3% 以内)、管の中の円柱(Schäfer–Turek 2D-1、Re 20。抗力係数 5.58 と 3% 以内。直径 10 セルで 5.66)。
- 渦の出る流れ: Schäfer–Turek 2D-2(Re 100)の円柱で、横の力の周期からストローハル数 0.298(基準 0.295〜0.305)。TRT の差の分の緩和時間 τ- を Λ = 3/16 から決めるだけだと、τ が 0.5 に近い時(τ 0.524 で τ- 8.3)に差の分がほとんど緩まず 3000 刻みほどで発散した。3D では入口の内側の速さが入口の 4 倍に出た。τ- の上限を 4 にして直した(BGK だと発散しない、で突き止めた。2D-1 は 5.67 のまま)。
- 通しの確かめは kmb-samples の `flow_tests.rs`(押し出した板の翼と尾翼、円柱の 2D の流れ、箱の 3D の流れ。寸法を変えて解き直す・色分けと凡例)。
- 渦格子法の翼幅の帯が粗いと、長方形の翼の e が 1 を少し超える(24 本で 1.03、48 本で 1.006、96 本で 0.995)。研究は 40 本で解く。
- ログ: `JC_LOG=info` で、流れの研究ごとに格子の大きさ・ボディのセル・壁の線の数・実際と計算した Re・解いた時間を出す(`流れ: …`、`流れ(…)`)。

## 12. 配る exe にこの PC のパスを残さない(`cargo xtask release`)

- アプリの release は `cargo xtask release` で作る(`--check` は `cargo check` だけ。`-p kmb-cli` などを付ければその crate)。`scripts/取り込み.ps1` もこれを使う。そのまま `cargo build --release` で作った exe には、次の 2 つからこの PC のパスが入る。
  - Rust: 依存の crate のソースの場所(`C:\Users\<名前>\.cargo\registry\src\...`)が、パニックの位置(`file!()`・`#[track_caller]`)として入る。自前の crate はワークスペースからの相対のパスなので入らない。
  - C と C++: SMLib のエラーの文の `FILE_NAME` や、cc で作る C の依存(lzma-sys の xz など)の assert の `__FILE__` が、ソースと見出しのパスのまま入る。MSVC は見出しを探した場所を絶対のパスにして入れる(PC で lzma-sys の `lzma_common.h` が 1 件残った。Linux の gcc は渡した相対のパスのままで入らない)。
- 直し方: Rust は rustc の `--remap-path-prefix` で、ホーム(`~`)・rustup(`rustup`)・cargo(`cargo`)・リポジトリ(`jc`)の頭を置き換える。cargo の `trim-paths` は 1.99 でも安定版で使えない(`feature trim-paths is required` で止まる)。PC ごとにパスが違うので `.cargo/config.toml` には書けず、release の時だけ xtask が `--config target.'cfg(all())'.rustflags=[...]` で渡す(debug の検査の組み立ては作り直さない)。C と C++ は、同じ頭を `CFLAGS`・`CXXFLAGS`(cc が読む)に足して消させる。MSVC は `/d1trimfile:<頭>\`(`__FILE__` の頭を切る、VS 2019 から)、gcc・clang は `-ffile-prefix-map=<頭>=<名前>`。cc は環境変数を空白で区切るので、空白を含むパスは渡さない。SMLib は smlib-sys の build.rs が、ソースと見出しをパッケージのフォルダからの相対のパスでも渡す。PDB の場所は rustc が既定で名前だけにする(`/PDBALTPATH:%_PDB%`)。
- 確かめ: `cargo xtask release` は作り直す前の exe と後の exe で、ホーム・リポジトリ・cargo と rustup の置き場所・target の文字列を数え(大文字と小文字を区別せず、UTF-8 と UTF-16LE の両方)、`パス: 前 N 件 → 後 M 件` を出す。後に 1 件でも残れば見本を出して失敗にする。配る前のファイルは `cargo xtask paths <ファイル>...` で数える。
- 環境変数 `RUSTFLAGS` か設定の `build.rustflags` を使っていると、置き換えの引数が効かない(cargo はどちらか 1 つしか使わない)。その時は数えた件数で気づく。
- `cargo xtask sweep` の release の問い合わせにも同じ引数と環境変数を付ける(付けないと別の指紋になり、今の exe の元を古い物として消す。cc は `CFLAGS` が変わるとビルドスクリプトを走らせ直す)。

## 13. 配る exe に他者の部品の許諾文を付ける(`cargo xtask licenses`)

- `cargo xtask release` は組み立てた後、exe の横(`target/release/THIRD-PARTY-LICENSES.txt`)に、配る物に入る他者の部品とその許諾文の一覧を置く。exe と一緒に配る。一覧だけは `cargo xtask licenses [--target <三つ組>] [--out <ファイル>]` で作る(既定は今の PC の向き)。コミットはしない(Cargo.lock から毎回作る)。
- 配る物: 層 `app` の crate(画面・CLI・MCP)から通常の依存だけをたどる。マクロ(proc-macro)とその先・build スクリプト用・試験用は exe に入らないので載せない。3 つの exe を 1 つの一覧でまかなう。
- 先頭は本体と自作の素材(アイコン・機械の台の穴のデータ・陰影の式など。本体と同じ MIT か Apache-2.0)。続いて crate でない部品(Rust の標準ライブラリ、SMLib・SM_API とその直しの写し、Hershey のフォント。`xtask/src/licenses.rs` の `EXTRAS`)、他者の crate。リポジトリから当てた写し(truck-shapeops)には、変えたことを注記する(Apache-2.0 の求め)。
- 本文は crate の一番上の LICENSE・COPYING・NOTICE などをそのまま載せ、同じ本文は 1 回だけにして使う部品を並べる。GPL の名前の付くファイル(GPL との二択の部品の物)は載せない。ファイルの無い crate は宣言の本文を `xtask/licenses/` の雛形から作る。雛形の無い許諾の crate が入ると `verify --all` と依存を変えた時の点検「許諾文の一覧」が落ちる。その時は許諾の本文を確かめ、雛形を足す(deny.toml の許す表に入っている物だけ)。

## 14. PR ごとの検査(GitHub の CI)

- `.github/workflows/verify.yml` が、PR と main へのプッシュのたびに `cargo xtask verify` を Linux(ubuntu-latest)と Windows(windows-latest)で流す。カーネルは SMLib(SMLib のソースは third_party にあり、そのまま組める)。
- 範囲: PR は分かれ目から変えた crate とそれに依存する crate(`--dependents --base origin/<PR の向き先>`)。検査の道具(`.github/workflows/`)・Rust の版(`rust-toolchain.toml`)・cargo の設定(`.cargo/`)・nextest の設定を変えた PR と、main へのプッシュ・手で流す時は全体(`--all`)。`--all` は記録の無い CI では truck の回も足す(1 節)。
- 道具の版は 1 節の入れ方とそろえる(cargo-nextest・cargo-deny・cargo-hakari)。
- 組み立て済みの物は main の回で残し、PR の回が使う(PR の中で残した物はほかの PR から使えない)。SMLib の C++ の結果は `SMLIB_CACHE_DIR`(作業場の外)に置き、別に残す(6 節)。
- 落ちた時は、その回の `target/verify/` のログを「verify-<OS>」の名前で取れる。
