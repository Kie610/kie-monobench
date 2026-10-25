# 試験のフォント

輪郭のフォントの文字(K31)の試験だけに使う小さなフォント。JC はフォントを同梱しないので、画面や
アプリでは使わない(PC のフォントを使う)。どちらも SIL Open Font License 1.1 のフォントから、試験に
使う字だけを抜き(fontTools の pyftsubset、ヒンティング無し)、名前を替えた物(OFL の決まりで、
抜いた物も OFL のまま。許諾と元の著作権の表示は `OFL.txt`)。

| ファイル | 元 | 字 | 形 |
|---|---|---|---|
| `jc-test-jp.ttf`(書体の名前 JC Test JP) | Noto Sans JP Regular(Google Fonts の配布) | 自作回の名入れ口 A O g o 8 と空白 | TrueType(2 次のベジエ) |
| `jc-test-cff.otf`(書体の名前 JC Test CFF) | Inter Regular(OS の fonts-inter の物) | A O g o 8 と空白 | CFF(3 次のベジエ) |

作り直す時(2026-10-22):

```
pyftsubset NotoSansJP-Regular.ttf --text="自作回の名入れ口AOgo8 " --no-hinting --desubroutinize \
  --layout-features='' --drop-tables+=BASE,STAT,gasp,prep,GSUB,GPOS,GDEF --output-file=jc-test-jp.ttf
```

その後 `name` 表の書体の名前(1・4・6)を替え、説明(5)に試験用と書く。CFF の物は CFF の名前も替える。
