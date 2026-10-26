### ツールバー(作業空間の箱・タブ・まとまり・画面が持つボタン・選択のまとまり)
### 操作の名前は action.ftl、欄で足す機能の名前は feature.ftl。

## 作業空間の箱

toolbar-space = 作業空間
toolbar-space-design = 設計
toolbar-space-machining = 加工
toolbar-space-render = レンダリング
toolbar-space-animation = アニメーション
toolbar-space-simulation = 解析
# $name: 今の作業空間の名前
toolbar-space-now = 作業空間: { $name }
toolbar-space-tip = 作業空間: 設計(形を作る)と加工(経路を作る)

## タブ

toolbar-tab-solid = ソリッド
toolbar-tab-sketch = スケッチ
toolbar-tab-render = レンダリング
toolbar-tab-animation = アニメーション
toolbar-tab-milling = フライス
toolbar-tab-simulation = 解析
toolbar-move-left = 左へ
toolbar-move-right = 右へ
toolbar-tab-order-reset = タブの順を既定に戻す

## まとまり(帯の下の名前)

toolbar-group-create = 作成
toolbar-group-modify = 修正
toolbar-group-construct = 構築
toolbar-group-pattern = パターン
toolbar-group-assemble = 組立
toolbar-group-inspect = 検査
toolbar-group-insert = 挿入
toolbar-group-export = 書き出し
# レンダリングの作業空間のまとまり
toolbar-group-setup = 設定
toolbar-group-canvas-render = キャンバス内レンダリング
toolbar-group-render = レンダリング
toolbar-group-transform = 変換
toolbar-group-view = ビュー
toolbar-group-publish = 公開
# 加工の作業空間のまとまり(材料と原点を決める段取り)
toolbar-group-cam-setup = セットアップ
toolbar-group-2d = 2D
toolbar-group-3d = 3D
toolbar-group-drilling = 穴
toolbar-group-check = 確かめる
toolbar-group-nc-program = NC プログラム
toolbar-group-output = 出力
toolbar-group-manage = 管理
toolbar-group-study = 研究
toolbar-group-flow = 流れ
toolbar-group-solve = 実行
toolbar-group-results = 結果
toolbar-group-constraints = 拘束
toolbar-group-dimension = 寸法
# スケッチから立体を作る道具のまとまり
toolbar-group-solid = 立体
toolbar-group-select = 選択
toolbar-group-select-tip = 選べる種類。ボディを入れると、面を押した時にそのボディごと選ぶ
toolbar-group-more = その他
toolbar-group-more-tip = 帯に入りきらないまとまり。窓を広げるか、ボタンの名前を出さない設定にすると帯に並ぶ
# $name: まとまりの名前
toolbar-collapsed = { $name }(畳んだまとまり)
# $name: まとまりの名前
toolbar-collapsed-tip = { $name }: 窓が狭いので 1 つに畳んだまとまり。押すと道具の一覧を出す

## 前に出すボタン(ピン)

toolbar-pin = 前に出す
toolbar-unpin = 前から外す
toolbar-pins-reset = 前の並びを既定に戻す
toolbar-group-reset = このまとまりを既定に戻す

## 画面が持つボタン

toolbar-measure = 計測
toolbar-measure-tip = M。点・辺・面をクリックして、距離・角度・径・面積を測る
toolbar-section = 断面解析
toolbar-section-tip = 平面で切って中を見る(形は変えない)。平らな面を選んでから押すと、その面に平行に切る
toolbar-press-pull = プレス/プル
toolbar-press-pull-tip = 選んだ物で決める: スケッチの形・平らな面は押し出し、辺はフィレット、フィレット・面取り・穴の面はその機能を直す
toolbar-scene = シーンの設定
toolbar-scene-tip = 環境の光(スタジオ・屋外)と明るさ・回転、背景(環境・単色・透明)、グラウンド面、カメラの焦点距離・露出・被写界深度、縦横比、絵と動画の大きさと長さ。設定の「レンダリング」
toolbar-appearance = 外観
toolbar-appearance-tip = A。ボディ・部品・面に外観(色・金属・粗さ)を付ける窓を開く。ライブラリを分類と名前で探し、このデザイン内の外観を直す
toolbar-canvas-render = キャンバス内レンダリング
toolbar-canvas-render-tip = 今のビューを光線追跡で描き、止まっている間に少しずつきれいにする(影と映り込み)。もう一度押すと止める
toolbar-params = パラメータ
toolbar-params-tip = Ctrl+P。右の編集パネルにパラメータの一覧を出す
toolbar-tools = 工具ライブラリ
toolbar-tools-tip = 工具の一覧を選ぶ(なければ足す)
toolbar-machine = 機械の設定
toolbar-machine-tip = 機械の値(作業範囲、回転数、コレット、ベッド)。設定の「機械」。複数の機械を持つマシンライブラリはまだ無い
# $name: 機能の名前。同じ名前のクリックで作る操作と見分ける
toolbar-feature-by-fields = { $name }(欄で指定)
# $key: 割り当てたキー(Ctrl+P など。訳さない)
toolbar-key = キー: { $key }
toolbar-key-not-here = この作業空間では使えないキーです

## 選択のまとまり

toolbar-pick-vertex = 頂点
toolbar-pick-edge = 辺
toolbar-pick-face = 面
toolbar-pick-body = ボディ
toolbar-pick-vertex-check = 頂点を選ぶ
toolbar-pick-edge-check = 辺を選ぶ
toolbar-pick-face-check = 面を選ぶ
toolbar-pick-body-check = ボディを選ぶ(面を押すとボディごと)
# $name: 選ぶのを待っている欄の名前
toolbar-pick-field = 欄「{ $name }」が受ける物
toolbar-pick-now-body = 面を押すとボディごと選ぶ
toolbar-pick-now-mixed = 頂点・辺・面を混ぜて選ぶ
toolbar-pick-all = すべて選べるようにする
