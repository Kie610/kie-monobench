### 3D のビューに重ねる部品(左上の案内、右上の等角と全画面、下の帯、表示のメニュー、ブラウザのつまみ、
### 計算に失敗した時の帯、加工の確かめの凡例)。機能・面の名前は文書の中身なので訳さない。

## 左上の案内

view-hint-select = 選択
# $count: 数
view-hint-vertices = { $count } 個の頂点
view-hint-edges = { $count } 本の辺
view-hint-faces = { $count } 面
# 頂点・辺・面の数を並べる時の区切り
view-hint-separator = ・
# $parts: 「2 個の頂点・1 本の辺」のような数の並び
view-hint-mixed = { $parts }を選んでいます。Shift・Ctrl+クリックで足す・外す
# $count: 面の数、$face: 最後に選んだ面の名前
view-hint-faces-many = { $count } 面を選んでいます(最後: { $face })。Shift・Ctrl+クリックで足す・外す
view-hint-face-one = 選んだ面: { $face }(Shift・Ctrl+クリックで足す)
# 解析の作業空間で、色分けした結果を出している時の見出し
view-hint-result = 結果
# 加工で、削る途中の形を出している時の見出し
view-hint-stage-shape = 途中の形

## 右上のボタン

view-fullscreen = 全画面表示
view-fullscreen-exit = 全画面表示を終了
view-iso = 等角
# もう一度押して反対側から見ている間の名前
view-iso-back = 等角(後)

## 下の帯

view-orbit = オービット
view-pan = 画面移動
view-zoom = ズーム
# $how: 今の回し方(view-orbit-constrained か view-orbit-free)
view-orbit-tip = 押している間、左ドラッグでオービット(もう一度押すか Esc で戻す)。今は{ $how }。右クリックで替える
view-pan-tip = 押している間、左ドラッグで画面移動(もう一度押すか Esc で戻す)
view-zoom-tip = 押している間、左ドラッグでズーム(もう一度押すか Esc で戻す)
view-orbit-constrained = 拘束オービット
view-orbit-free = フリーオービット
view-orbit-constrained-item = 拘束オービット(上下を保つ)
view-fit = 全体表示
view-fit-tip = 全体を収め、回す中心を部品の中心にする(Home)
view-look-at = 正面
view-look-at-tip = 選んだ面・スケッチ・原点の平面を真正面から見て、形全体を収める(Look At)
# $why: 使えない理由(view-look-at-no-target など)
view-look-at-why = 選んだ面・スケッチ・原点の平面を真正面から見る。{ $why }
view-look-at-no-target = 先に面・スケッチ・原点の平面を選んでください
view-look-at-no-normal = この面の向きが分かりません
view-display = 表示 ▾
view-display-tip = ブラウザ・タイムライン・編集パネル、表示の形と投影、格子と原点

## 表示のメニュー

view-menu-panels = パネル
view-menu-data-panel = データパネル
view-menu-data-panel-tip = 最近・プロジェクトのデザインの一覧(上の帯の左端のボタンと同じ)
view-menu-browser = ブラウザ
view-menu-timeline = タイムライン(加工は段の帯)
view-menu-timeline-off = レンダリングと解析では出しません(設計へ戻すと出ます)
view-menu-panel-always = 選んだだけでも編集パネルを出す
view-menu-panel-always-tip = 切: 編集パネルは作る・直す・パラメータの間だけ出す。入: 機能を選ぶと値を見せる
view-menu-commands = コマンド
view-menu-commands-tip = コマンドを打つ欄。画面の部品を名前で押す・打つ、スクリプトを流す(help で一覧)
view-menu-reset-dock = 並びを既定に戻す
view-menu-reset-dock-tip = 部品のタブを引いて付け替えた並びを、初めの形に戻す(隠した部品も出す)

## 表示スタイル・投影・目安・効果(表示のメニュー、何もない所の右クリック、設定の窓)

view-style = 表示スタイル
view-shading-shaded-edges = シェーディング、エッジ表示のみ
view-shading-shaded-hidden = シェーディング、隠線エッジ表示
view-shading-shaded = シェーディング
view-shading-wireframe = ワイヤフレーム
view-projection = 投影
view-projection-parallel = 平行投影
view-projection-parallel-tip = 遠近をつけず、平行な線は平行に見せる
view-projection-perspective = 透視投影
view-projection-perspective-tip = 遠くの物ほど小さく見せる
view-projection-auto = 自動
view-projection-auto-tip = 上・前・右など X・Y・Z の軸の向きから見る時は平行投影、それ以外の向きでは透視投影にする
view-guides = 目安
view-grid = グリッド
view-grid-tip = XY 平面の格子。間隔は拡大に合わせて変わる(左下に今の間隔)
view-origin = 原点と軸
view-origin-tip = 原点と X(赤)・Y(緑)・Z(青)の軸。空の文書では基準面も
view-see-through = 全部透かす
view-see-through-tip = 本体の奥にあるスケッチと作業平面も普通の線で見せる。切ると、隠れた所は薄い点線
view-effects = 効果
view-materials = 材質の質感
view-materials-tip = 外観・物理材料を付けたボディを、材質の見た目で描く(木目、金属の映り込み、つや)。レンダリングの作業空間では、いつも全部のボディをこの描き方にする
view-env-studio = スタジオ(明るい)
view-env-studio-dark = スタジオ(暗い)
view-env-outdoor = 屋外

## ブラウザを隠した時の左の端のつまみ

# 縦に 1 字ずつ並べる。短い語にする
view-grip = ブラウザ
view-grip-tip = ブラウザを出す

## ビューの上の知らせと凡例

# $names: 計算に失敗した機能の名前(「押し出し1・フィレット2」)
view-held = { $names }の計算に失敗したので、前に計算できた形を出しています
# $count: 印の数
view-mark-gouge = 赤の点: 食い込み・工具の当たり { $count } か所
view-mark-leftover = 紫の点: 削り残し { $count } か所(大きいほど厚い)

## 重なった面の候補(長押し)

view-candidates = 重なっているもの(手前から)

view-candidate-edge = 辺({ $faces })
view-candidate-vertex = 頂点({ $position })

view-others = ほかを選ぶ…(長押しでも)
view-others-help = 右クリックした所に重なっている面・辺・頂点の一覧から選びます。左ボタンの長押しでも開けます。
