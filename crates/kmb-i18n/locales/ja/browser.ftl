### ブラウザ(左の木。設計・加工・解析の木、種類の札、行の目と丸いボタン、右クリック)
### 木のまとまりの名前は開け閉めの覚えの鍵にも使うので、鍵は変えずに出す時だけ訳す。

browser-title = ブラウザ
browser-hide = ブラウザを隠す
browser-hide-tip = ブラウザを隠す(左の端の「ブラウザ」で戻す)
browser-search = 名前で探す

## 種類の札

browser-kind-all = すべて
browser-kind-sketch = スケッチ
browser-kind-body = ボディ
browser-kind-params = パラメータ
browser-kind-operation = 操作
browser-kind-tools = 工具
browser-kind-other = そのほか

## 行の目と丸いボタン($name: 行の名前)

browser-eye-hide = { $name } を隠す
browser-eye-show = { $name } を出す
browser-eye-hide-tip = 3D のビューで隠す(計算と加工には使う)
browser-eye-show-tip = 3D のビューに出す
browser-active = { $name } はアクティブ
browser-activate = { $name } をアクティブにする
browser-active-tip = アクティブ: 新しく作るボディと部品はこの中に入る
browser-activate-tip = アクティブにする: 新しく作るボディと部品をこの中に入れ、ほかの部品を淡く出す

## 設計の木

# 文書にまだ名前が無い時の、文書の行
browser-untitled = (無題)
browser-root-activate = アクティブにする
browser-root-activate-tip = 文書の直下をアクティブに戻す(新しく作るボディは部品に入らない)
browser-properties = 設計の属性…
browser-origin = 原点
browser-plane-xy = XY 平面
browser-plane-yz = YZ 平面
browser-plane-zx = ZX 平面
browser-axis-x = X 軸
browser-axis-y = Y 軸
browser-axis-z = Z 軸
browser-named-views = 名前の付いたビュー
browser-save-view = 今のビューを保存
browser-motion-studies = モーション スタディ
browser-contact-sets = 接触セット
# $names: ボディの名前を「・」でつないだ物
browser-contact-bodies = ぶつかり合うボディ: { $names }
browser-contact-delete = 削除
browser-params = パラメータ
browser-params-tip = Ctrl+P。右の編集パネルに一覧を出す
browser-folder-sketch = スケッチ
browser-folder-body = ボディ
browser-folder-other = そのほかの機能
browser-cam-elsewhere = 加工の操作 { $count } 個は作業空間「加工」で出します
browser-expand-all = 下を全部開く
browser-expand-all-tip = この行と、下の枝をすべて開く
browser-collapse-all = 下を全部閉じる
browser-collapse-all-tip = 下の枝をすべて閉じる(この行は開いたまま、すぐ下の行だけが並ぶ)
# 畳んだまとまりの中に注意・エラーがある時
browser-inside-warning = 中に注意があります(開くと見える)
browser-inside-error = 中にエラーがあります(開くと見える)
browser-inside-blocked = 中に計算していない物があります(開くと見える)

## 解析の木

browser-studies = 研究
browser-studies-empty = まだありません。上の「静的応力」「熱」「固有振動」「軽い形の参考」「翼の揚力」「2D の流れ」「3D の流れ」から足す

## 加工の木

browser-machine = 機械
browser-machine-tip = 機械の値は設定の「機械」で直す
browser-machine-settings = 機械の設定
browser-setups = セットアップ
browser-setups-none = 使える操作はありません
browser-new-setup = 新しいセットアップ
browser-setups-empty = まだありません。上の「セットアップ」から足す
browser-fixture = 固定具
browser-clamps = クランプ { $count }
browser-orphans = セットアップのない操作
browser-tools = この文書の工具
