# 画面を撮って確かめる

画面に関わる変更は、実際の画面を撮って画面の試作や前の版と並べて確かめる。撮り方は 2 つあり、用途で使い分ける。

| 方法 | 向いていること | 要るもの |
|---|---|---|
| `apps/kmb-app/tests/screens.rs`(既定の場面を撮る試験) | 決まった場面を同じ条件で撮る。PC(Windows)でもクラウドでも動き、追加のパッケージは要らない。egui が出す三角形を CPU で塗って BMP にする | `cargo` だけ(日本語のフォントが見つからない Linux では `JC_FONT`) |
| `scripts/ui-shot/drive.sh`(実際のアプリを動かして撮る) | クリック・二度押し・キーなどの操作の流れを追い、実際のアプリ(wgpu の描画)の見た目を撮る | Linux の Xvfb、xdotool、ImageMagick、mesa-vulkan-drivers(lavapipe)、libxkbcommon-x11-0、fonts-ipafont-gothic |

- 決まった場面: `JC_SCREENS_OUT=out cargo test -p kmb-app --test screens -- --ignored`
- 操作の流れ: `cargo build -p kmb-app` のあと `SHOT_OUT=out scripts/ui-shot/drive.sh scripts/ui-shot/screens.steps --sample`(暗い画面は `SHOT_THEME=dark`)。手順の書き方は drive.sh の先頭を見る。

- 利用者の PC と同じ大きさと倍率で撮る: Windows の表示倍率 200%・4K(3840×2160、作業バー抜きで 3840×2068)なら、決まった場面は `JC_SCREENS_SIZE=1920x1034 JC_SCREENS_SCALE=2`、操作の流れは `SHOT_SIZE=3840x2068 SHOT_SCALE=2`。
- PC の文字の見え方を真似る: クラウドの IPA P ゴシックは行の高さ 1.0 em、Windows の Yu Gothic Medium は 1.6 em と違う。アプリは読み込む時に行の高さを揃える(crates/kmb-ui/src/fonts.rs)ので、揃える前の崩れを確かめたい時だけ、縦の寸法を書き換えたフォントを `JC_FONT` で渡す。

- 人の手順で 1 手ずつ点検する: `scripts/ui-shot/session.sh 1 start` で立て、`click`・`drag`・`key`・`shot` などを 1 手ずつ送り、写真を見て次を決める。台の番号ごとに画面と HOME が分かれるので、番号を変えれば何台でも同時に動かせる(CPU 4 個の作業場で 3 台まで確認)。

新しい場面を足す時は、まず screens.rs に足す(再現しやすい)。マウスの操作でしか出ない画面だけ drive.sh の手順に足す。
