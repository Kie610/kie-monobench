### 機能の注意・エラーを確かめる窓(ビューの右下)と、右下の角の数の札。
### 知らせの文そのものと直し方の例は、知らせと誤りの文の分野で訳す。機能の名前は訳さない。

review-ok = 今は計算できています
review-close = 閉じる
review-close-tip = Esc でも閉じる
# $name: 計算を止めている前の機能の名前
review-blocked-by = 止めている機能: { $name }
review-fixes = 直し方の例
# 内部の言葉を残した元の文(不具合の報告に使う)の見出し
review-raw = 元の文
# 写す文の 2 行目。$raw: 元の文
review-raw-line = 元の文: { $raw }
review-no-problem = この機能に注意やエラーはありません
review-edit = 編集
review-copy = 文を写す
review-copy-tip = 機能の名前と知らせの文(元の文も)をクリップボードへ
review-pick-cause = 原因のフィーチャを選ぶ
review-copied = 写しました

## 右下の角の札

# 札の字。$errors・$warnings: 数(どちらかは 1 以上)
review-badge =
    { $errors ->
        [0] 注意 { $warnings }
       *[other] { $warnings ->
            [0] エラー { $errors }
           *[other] エラー { $errors }・注意 { $warnings }
        }
    }
# 札の吹き出し。$badge: 札の字(review-badge)
review-badge-tip = { $badge } があります。押すと確かめる窓が開き、もう一度押すと次の機能へ進む
