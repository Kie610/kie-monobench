### 画面の外枠(窓のボタン・上の帯・知らせ・文書のタブ・下の帯)

## 窓のボタン(アプリが描く窓の帯の右端)

frame-window-minimize = 最小化
frame-window-restore = 元の大きさに戻す
frame-window-maximize = 最大化
frame-window-close = 閉じる

## 上の帯

frame-data-panel = データパネル
frame-data-panel-hide = データパネルを隠す
frame-data-panel-show = データパネルを表示: 最近・プロジェクトのデザインを左に並べ、開く・3D のビューへ引いてリンクで挿入する
frame-save = 保存
frame-save-tip = 保存(Ctrl+S)
frame-undo = 元に戻す
frame-undo-tip = 取り消し(Ctrl+Z)
frame-redo = やり直し
frame-redo-tip = やり直し(Ctrl+Y)
frame-undo-history = 取り消しの履歴
frame-undo-history-tip = 取り消しの履歴: 済んだ変更の一覧。行を押すと、その時点へ戻る
frame-settings = 設定
frame-settings-tip = 設定: マウス、画面の配置、機械など、この PC 全体の設定
frame-help = ヘルプ
# 検索の欄の形をしたボタンに薄く出す字
frame-search-placeholder = コマンドを探す
frame-search = コマンド検索
frame-search-tip = 名前や読みでコマンドを探して実行する(Ctrl+K)

## 知らせ(鐘のボタンと一覧)

# $count: 知らせの数
frame-notices = 知らせ({ $count })
frame-notices-tip = 知らせ: 計算の注意、保存、書き出しの結果
# $count: 知らせの数
frame-notices-heading = 知らせ ・ { $count } 件
frame-notices-empty = 知らせはありません
frame-notices-copy = 写す
frame-notices-copy-tip = この知らせをクリップボードへ写す
frame-notices-copy-all = 全部を写す
frame-notices-copy-all-tip = 知らせを古い順に、時刻つきでクリップボードへ写す
frame-notices-clear = 履歴を消す

## 文書のタブ

frame-tab-untitled = (無題)
# $path: 文書のファイルの場所
frame-tab-saved = { $path }
# $path: 文書のファイルの場所
frame-tab-saved-modified = { $path }(未保存)
frame-tab-unsaved = まだ保存していない文書
frame-tab-unsaved-modified = まだ保存していない文書(未保存)
frame-tab-close = 閉じる
frame-tab-close-others = ほかのタブを閉じる
frame-tab-more = ほかのタブ
# $count: 帯に入らないタブの数
frame-tab-more-tip = 帯に入らないタブ { $count } 枚(押すと一覧)
frame-tab-new = 新しいタブ
frame-tab-new-tip = 新規デザインを別のタブで開く(Ctrl+N)

## 下の帯

# $unit: 長さの単位の名前(mm・in など)
frame-status-unit = 単位 { $unit }
frame-status-unit-tip = この文書の長さの単位。押すと設定の「単位」で変えられる
frame-status-recalculating = 再計算中
# $secs: 経った秒数
frame-status-recalculating-secs = 再計算中 { $secs } 秒
frame-status-recalculating-tip = 計算の間もビューを回したり選んだりできる。終わると形を差し替える
frame-status-computing = 計算中
# $secs: 経った秒数
frame-status-computing-secs = 計算中 { $secs } 秒
frame-status-computing-tip = 計算の間もビューを回したり選んだりできる。終わると結果を知らせる
# $note: 仕事の名前と進み、$elapsed: 経った時間(例 1:05)
frame-status-progress = { $note } · 経過 { $elapsed }
# $note: 仕事の名前と進み、$elapsed: 経った時間、$left: 残りの見込み
frame-status-progress-left = { $note } · 経過 { $elapsed } · 残り 約 { $left }
frame-status-finish-tip = ここまでの回数で絵を仕上げてファイルに書く(粗さが残ることがある)
frame-status-stop = 中止
frame-status-stop-tip = 途中でやめる。ファイルは書かない
frame-status-dismiss-tip = この知らせを消す(鐘の一覧には残る)
# $count: 選んでいる物の数
frame-status-selected = { $count } 個を選んでいます
frame-status-files-local = ファイルはこの PC に保存します。ネット接続は要りません。

## 下の帯の操作の案内(左がキーやボタンの名前、右がその働き)

frame-key-left = 左
frame-key-right = 右
frame-key-digits = 数字
frame-key-middle = ホイール押し
frame-key-wheel = ホイール
frame-hint-ok = OK
frame-hint-click-to-set = クリックで決める
frame-hint-type-value = 値を打つ
frame-hint-cancel = キャンセル
frame-hint-group = グループ
frame-hint-delete = 削除
frame-hint-select = 選ぶ ・ ドラッグで範囲 ・ 長押しで重なり
frame-hint-menu-orbit = メニュー ・ ドラッグで回す
frame-hint-middle-pan = 動かす ・ 二度押しでそこを回す中心に
frame-hint-middle-orbit = 回す ・ 二度押しでそこを回す中心に
frame-hint-zoom = 拡大
