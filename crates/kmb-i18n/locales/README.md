# 画面の文言の訳 / Translations

言語ごとのフォルダ(`ja`・`en`・`zh-Hans`・`zh-Hant`・`ko`)に、分野ごとの Fluent のファイル(`.ftl`)を置く。
日本語と英語は全部の ID を持つ。ほかの言語は訳した ID だけを書けばよく、無い ID は英語で出る。

One folder per language, one Fluent file (`.ftl`) per area. Japanese and English contain every ID.
Other languages only need the IDs that have been translated; missing ones fall back to English.

## 訳を足す / Contributing a translation

1. 英語(`en/<分野>.ftl`)の行と同じ ID で、訳す言語のファイルに `ID = 訳` を書く。
   Copy the ID from `en/<area>.ftl` and write `id = translation` in your language's file.
2. `{ $name }` は差し込み口。名前を変えずに、語順に合わせて置く。`#` の行は訳す人への注記。
   `{ $name }` is a placeholder: keep its name, move it where your grammar needs it. `#` lines are notes for translators.
3. 数で形が変わる語は英語の書き方(`{ $count -> ... }`)に倣う。形の変わらない言語は 1 つの文でよい。
   For plurals follow the English selector; languages without plural forms can use a single sentence.
4. `cargo test -p kmb-i18n` で、ファイルが読めることと ID が英語にあることを確かめて、PR を送る。
   Run `cargo test -p kmb-i18n` to check the files parse and use known IDs, then open a pull request.

## ID の決まり / ID rules

- 小文字とハイフン。頭は分野(`settings-`・`common-`)。 Lowercase with hyphens, prefixed by the area.
- 置き場ごとに別の ID(同じ字でも訳し分けることがある)。`common-` は働きまで同じボタンだけ。
  One ID per place in the UI; `common-` is only for buttons that behave identically everywhere.
- 意味が変わったら ID を替える。字の誤りの直しだけなら替えない。
  Change the ID when the meaning changes; keep it for typo fixes.
- 文を切って継ぎ合わせない。 Never build sentences by joining fragments.

## 操作と機能の名前 / Command and feature names

- `action.ftl` の ID は操作の ID の `.` と `_` を `-` にした物(`cad.extrude` → `action-cad-extrude`)。`feature.ftl` は機能の種類から版を外した物(`solid.hole@1` → `feature-solid-hole`)。説明は後ろに `-tip`。
  `action.ftl` IDs come from command IDs (`cad.extrude` → `action-cad-extrude`); `feature.ftl` IDs from feature types without the version (`solid.hole@1` → `feature-solid-hole`). Descriptions add `-tip`.
- 日本語の文はモジュールが付けた名前と同じにする(試験が比べる)。名前を変える時はモジュールと `ja/` を一緒に直す。
  The Japanese text must match the module's own name (a test compares them).

## 編集パネルの欄 / Edit panel fields

- `field.ftl` の ID は `field-<種類の版を外した物>-<欄の鍵>`。`.` と `_` は `-` にする。行の中の欄は親の鍵も含める(`edges.radius` → `edges-radius`)。説明は後ろに `-help`。
  Field IDs use the feature kind without its version and the complete field key. Replace dots and underscores with hyphens. Include the parent key for row fields; append `-help` for help text.
- 日本語は記述子の欄の名前・説明と同じにする。専用の編集器で `Field` を描く時は、`kmb_ui::translated_field` で同じ訳を引ける。訳の無い拡張の欄は、記述子の既定の字で出す。
  Japanese must match each descriptor's label and help. Custom editors rendering a descriptor field can use the same translations through `kmb_ui::translated_field`. Fields without translations retain their descriptor defaults.
