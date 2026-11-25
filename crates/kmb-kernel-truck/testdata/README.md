# 試験用の STEP

| ファイル | 中身 |
| --- | --- |
| `reference.step` | 基準部品(`kmb_solid::sample::reference_part` と同じ寸法)をほかの CAD で作って書き出したもの。作った CAD の値: 体積 22806.876 mm³、表面積 7678.499 mm² |
| `reference-fillet.step` | 上に、外周の縦の角 4 本の半径 2 mm のフィレットを足したもの。作った CAD の値: 体積 22772.540 mm³、表面積 7637.296 mm² |
| `cone-point.step`・`cone-point-vertex.step`・`cone-point-seam.step` | 先が 1 点にすぼまる円錐(底の半径 8 mm・高さ 12 mm、体積 256π = 804.248 mm³)。円錐の面の縁が底の円だけの物、底の円と頂点の VERTEX_LOOP の物、底の円と継ぎ目の直線の物。2026-10-22 に JC の STEP 書き出しの頭と底の面を使い、円錐の面(CONICAL_SURFACE)を手で書いた |
| `cylinder-replica.step` | 円柱(半径 10 mm・高さ 20 mm、体積 2000π = 6283.185 mm³)。上の縁の円を、下の縁の円の写し(CURVE_REPLICA。Z のまわりに 90 度回して 20 mm 上げる CARTESIAN_TRANSFORMATION_OPERATOR_3D、名前と説明つきの 7 つの引数)として書いた。2026-11-25 に上の円錐と同じ頭で手で書いた |

上の 2 つは 2026-10-12 に作り、STEP(AP214)で書き出した。
