# kieMonoBench に加わる

日本語 · [English](CONTRIBUTING.en.md)

不具合の知らせ・要望・PR を受け付けます。返事の早さは約束できません(作っているのは少人数です)。

## 始める前に

- **作る前に Issue で相談してください。** 小さな直し(誤字・明らかな不具合)は相談なしで PR してかまいません。
- 思想・構造・入出力の約束・見た目と作業の決まりは [AGENTS.md](AGENTS.md) にあります。分野の細かい決まりは [.claude/rules/](.claude/rules/) に、どこに何があるかは [docs/地図.md](docs/地図.md) にあります。人も AI も同じ決まりで作ります。
- 脆弱性は公開の Issue に書かず、[SECURITY.md](SECURITY.md) の手順で知らせてください。

## ビルドと検査

ビルドに要る物は [README.md](README.md#ソースからビルドする) にあります。PR の前に、変えた crate の検査を流してください。

```sh
cargo xtask verify                # 変えた crate のテストと点検
cargo xtask verify --dependents-lower   # 変えた crate に依存する crate も(組み立てと画面・アプリの手前まで)
```

PR の本文には、検査の結果を件数(通過・失敗・飛ばし)で書いてください。点検(層の向き・引数の宣言・言葉の揺れなど)の意味は [docs/verify.md](docs/verify.md) にあります。点検が落ちたら、点検を緩めずに直してください。

## PR の決まり

- 1 つの PR は 1 つの目的にします。ついでの整形や作り替えは別の PR にしてください。
- 画面を変えたら、変える前と後の写真を並べて付けてください。
- 機能を足したら、記録を `docs/機能/` に 1 機能 1 ファイルで書いてください。書き方は [docs/機能/README.md](docs/機能/README.md) にあります。
- 資料は [.claude/rules/docs.md](.claude/rules/docs.md) の書き方で書いてください。
- 新しい依存は MIT・Apache-2.0・BSD などの緩いライセンスに限ります(`deny.toml`)。**GPL・LGPL・AGPL の依存とコードは使えません。**
- ほかのソフトのコードは、ライセンスに関わらず写したり訳したりしないでください。仕組みを知るために読むのはかまいませんが、自分の言葉で書いてください。
- コード・コメント・資料に、ほかの CAD・CAM の製品の名前や調べた資料の出典を書かないでください(互換に要る機械と形式の名前は除く)。**PR の本文・コミットの説明・ブランチ名も公開されます。**
- 特許のある操作の形は作りません。作らない形の一覧は [docs/避ける形.md](docs/避ける形.md) です。

## 取り込み

PR は維持する人が読み、手元で全体の検査(`cargo xtask verify --all`)を通してから main に入れます。GitHub の画面の merge ボタンは使いません。届いた Issue と PR の仕分けと返し方は [docs/受け方.md](docs/受け方.md) にあります。

## ライセンス

あなたが取り込みのために出した物は、特に断らない限り、Apache-2.0 の定める「Contribution」として、このリポジトリと同じく MIT と Apache-2.0 の両方で配られ、追加の条件は付きません。署名の手続き(CLA)は求めません。製品名とアイコンの扱いは [TRADEMARKS.md](TRADEMARKS.md) にあります。

## 振る舞い

[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) に従ってください。
