### タイムライン(下の機能の列。具合の印と吹き出し、ロールバックの棒、グループ、機能の右クリック)
### 機能とグループの名前は文書の中身なので訳さない。注意・エラーの文そのものは知らせと誤りの文の分野で訳す。

## 計算の具合

timeline-health-warning = 注意
timeline-health-error = エラー
timeline-health-blocked = 計算していない
timeline-health-ok = 計算できた
# $by: 止めている前の機能の名前
timeline-health-blocked-by = { $by } が使えないので、この機能は計算していません
timeline-status-blocked = { $by } が使えません
timeline-status-suppressed = 抑制中
timeline-status-rolled-back = ロールバック位置より後ろ

## 札の吹き出し($name: 機能の名前、$title: 具合の種類、$message: 知らせの文)

timeline-tip-error =
    { $name }
    { $title }: { $message }
    右クリックの「エラーを確かめる」で詳しく、「文を写す」で写せる
timeline-tip-warning =
    { $name }
    { $title }: { $message }
    右クリックの「注意を確かめる」で詳しく、「文を写す」で写せる
# $value: 小数 1 桁の秒数
timeline-seconds = { $value } 秒
# $time: かかった時間(「12 ms」「1.5 秒」)
timeline-calc-time = 計算 { $time }
timeline-calc-reused = (前の結果を使い回し)
timeline-chip-hint = ダブルクリックで編集・ドラッグで並べ替え・右クリックでほかの操作

## 列と下の行

timeline-empty = 機能はまだありません。上のツールバーから作ります
timeline-rollback-start = ロールバックの棒を先頭へ
timeline-rollback-back = ロールバックの棒を 1 つ前へ(後ろの機能を一時的に無効にする)
timeline-rollback-forward = ロールバックの棒を 1 つ後ろへ
timeline-rollback-end = ロールバックの棒を最後へ(すべて有効にする)
timeline-marker = 今の位置の印
timeline-count = { $count } 機能
timeline-count-errors = エラー { $count }
timeline-count-warnings = 注意 { $count }
timeline-count-groups = グループ { $count }
timeline-count-rolled = { $count } つはロールバックの後
# 引いている機能が 2 つ以上の時
timeline-drag-many = { $count } 個の機能
timeline-inside-warning = 中に注意があります(開くと見える)
timeline-inside-error = 中にエラーがあります(開くと見える)
timeline-inside-blocked = 中に計算していない物があります(開くと見える)
timeline-find-in-browser = ブラウザ内を検索
timeline-find-in-browser-tip = この機能のボディを、ブラウザで光らせる

## グループ($name: グループの名前)

timeline-group-expand = グループ「{ $name }」を開く
timeline-group-collapse = グループ「{ $name }」を畳む
timeline-group-expand-tip =
    グループ「{ $name }」({ $count } 個)
    押すと開く。右クリックで名前変更・解く・消す
timeline-group-collapse-tip =
    グループ「{ $name }」({ $count } 個)
    押すと畳む。右クリックで名前変更・解く・消す
timeline-group-name = グループの名前
timeline-group-select-tip = 押すと中身をまとめて選ぶ。右クリックで名前・解く・消す
timeline-group-cannot = グループにできません
timeline-group-include-gaps = 間も含める
timeline-group-include-gaps-tip = 最初から最後までの間にある機能も入れてまとめる
timeline-group-gather = 寄せてからまとめる
timeline-group-gather-tip = 間の機能を後ろへ出し、選んだものだけを並べてまとめる
timeline-group-rename = 名前変更
timeline-group-open = 開く
timeline-group-close = 畳む
timeline-group-suppress = まとめて抑制
timeline-group-unsuppress = まとめて抑制を解除
timeline-ungroup = グループを解く(中身は残す)
timeline-group-delete = グループと中身を削除
timeline-group-delete-tip = 中の { $count } 個の機能も消す。取り消し(Ctrl+Z)で戻せる

## 機能の右クリック(タイムライン・ブラウザ・3D のビューで同じ)

timeline-menu-review-error = エラーを確かめる
timeline-menu-review-warning = 注意を確かめる
timeline-menu-review-tip = ビューの右下に、知らせの全文と直し方の手がかりを出す
timeline-menu-copy = 文を写す
timeline-menu-copy-tip = 機能の名前と知らせの文をクリップボードへ(質問や報告に貼る)
timeline-menu-select-cause = 原因のフィーチャを選ぶ
timeline-menu-select-cause-tip = 止めている前の機能を選ぶ
timeline-menu-edit = フィーチャを編集
timeline-menu-edit-tip = 編集パネルでこの機能を直す(ダブルクリックでも開く)
timeline-menu-edit-many = { $count } 個をまとめて編集
timeline-menu-edit-many-tip = 共通の欄を、選んだ全部に入れて直す(同じ種類の機能を選ぶと直せる)
timeline-menu-edit-sketch = スケッチを編集
# $name: スケッチの名前
timeline-menu-edit-sketch-of = 元のスケッチ「{ $name }」を直す
timeline-menu-edit-sketch-tip = この機能が使うスケッチ「{ $name }」を開いて直す
timeline-menu-rename = 名前変更
timeline-menu-rename-tip = タイムラインとブラウザに出る名前を変える(取り消せる)
timeline-menu-suppress = 抑制
timeline-menu-unsuppress = 抑制を解除
timeline-menu-roll-back = ここまで戻す
timeline-menu-roll-back-tip = この機能より後ろを一時的に無効にする(後ろの機能は消えない)
timeline-menu-roll-forward = 最後まで進める
timeline-menu-move-up = 上へ
timeline-menu-move-down = 下へ
timeline-menu-group = グループにする({ $count } 個)
# $name: グループの名前
timeline-menu-take-out = 「{ $name }」から出す
timeline-menu-copy-feature = コピー
timeline-menu-copy-feature-tip = 値と式を写す。貼る時に面や辺を選び直す
timeline-menu-paste-feature = 貼り付け
timeline-menu-paste-feature-tip = タイムラインの最後に貼り、面や辺を選び直す
timeline-menu-paste-feature-empty = 先に機能をコピー(Ctrl+C)する
timeline-menu-delete = 削除
timeline-menu-delete-tip = 取り消し(Ctrl+Z)で戻せる
