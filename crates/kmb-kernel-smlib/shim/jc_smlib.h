/* SMLib(usd-brep の SM_API)を Rust から呼ぶための、C の薄い中継層。
 *
 * - 形状は JcBrep(中身は SmBrep)への不透明なポインタで渡す。持ち主は呼び出し側で、
 *   jc_brep_free で消す。ブーリアンは道具の形状を消費する(成功・失敗にかかわらず中継層が消す)。
 * - 戻り値の int は 0 が成功。失敗の理由は jc_last_error() で読める(呼び出したスレッドごと)。
 * - 面の名前はカーネルに持たせず、面ごとの整数のタグ(SMLib の属性)だけを持たせる。
 *   タグと名前の対応は Rust 側が持つ。
 * - SMLib は 1 つの文脈を全体で共有するので、呼び出しは 1 スレッドずつにすること(Rust 側で錠をかける)。
 */
#ifndef JC_SMLIB_H
#define JC_SMLIB_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct JcBrep JcBrep;

/* 輪郭の 1 区間。座標はスケッチの平面の 2D 座標。角度はラジアンで、円弧は start から end へ反時計回り。 */
enum { JC_SEG_LINE = 0, JC_SEG_ARC = 1, JC_SEG_CIRCLE = 2 };
typedef struct JcSegment {
    int32_t kind;
    double a[2], b[2];      /* 直線の両端 */
    double center[2];       /* 円弧・円 */
    double radius;
    double start, end;      /* 円弧 */
} JcSegment;

/* 平面: origin, x_axis, y_axis(法線は x × y)。 */
typedef struct JcPlane {
    double origin[3], x_axis[3], y_axis[3];
} JcPlane;

/* 面の情報。jc_faces の並びは、形状が変わらない限り同じ。 */
typedef struct JcFace {
    int64_t tag;            /* タグが無ければ -1 */
    int32_t planar;         /* 平面なら 1 */
    double normal[3];       /* 平面の外向きの法線(planar のときだけ) */
    double plane_point[3];  /* 平面の上の点(planar のときだけ。面の外のこともある) */
    double point[3];        /* 面の内側の点(SmApiFaceInternalPoint) */
    double area;
} JcFace;

/* 2 つの平面の境目の、まっすぐな辺。f0, f1 は jc_faces の添字。 */
typedef struct JcEdge {
    uint32_t f0, f1;
    double a[3], b[3];
    int32_t convex;
} JcEdge;

/* 平面と、それに垂直な円筒の境目の円(弧)の辺。f0 が平面、f1 が円筒。
 * a から軸(平面の外向きの法線)まわりに sweep ラジアン回ると b(一周なら a == b、sweep = ±2π)。
 * inside は平面の面が円の内側にあるか(ボスの上面なら 1、穴の縁なら 0)。 */
typedef struct JcArcEdge {
    uint32_t f0, f1;
    double center[3], axis[3];
    double radius, sweep;
    double a[3], b[3];
    int32_t inside;
    int32_t convex;
} JcArcEdge;

/* has_centroid = 0 なら重心は求められなかった(閉じた立体でないなど)。 */
typedef struct JcMassProps {
    double volume, area;
    double min[3], max[3];
    double centroid[3];
    int32_t has_centroid;
} JcMassProps;

/* 三角形の網。tri_face は jc_faces の添字。edge_counts[i] 個ずつの点が edge_points に並ぶ。 */
typedef struct JcMesh {
    size_t vertex_count;
    float* positions;       /* vertex_count * 3 */
    float* normals;         /* vertex_count * 3 */
    size_t triangle_count;
    uint32_t* triangles;    /* triangle_count * 3 */
    uint32_t* tri_face;     /* triangle_count */
    size_t edge_count;
    uint32_t* edge_counts;  /* edge_count */
    float* edge_points;     /* 合計点数 * 3 */
} JcMesh;

const char* jc_last_error(void);

/* 輪郭(閉じたループの集まり)を平面の上の面にし、法線方向へ from から to まで押し出す。
 * loop_lens[i] 個ずつの区間が segments に並ぶ。外周と穴の区別は SMLib が入れ子から決める。 */
typedef struct {
    int32_t kind; /* 0: line, 1: interpolating points, 2: arc, 3: body edge */
    uint32_t count;
    const double* points;
    const JcBrep* brep;
    uint32_t f0, f1;
    double center[3], axis[3], sweep;
} JcBoundary;
int jc_planar_sheet(const JcPlane*, const JcSegment*, const uint32_t*, size_t, JcBrep**);
int jc_patch_sheet(const JcBoundary*, size_t, int32_t, JcBrep**);
int jc_is_sheet(const JcBrep*, int32_t*);

int jc_extrude(const JcPlane* plane, const JcSegment* segments, const uint32_t* loop_lens,
               size_t loop_count, double from, double to, JcBrep** out);

/* 輪郭を法線方向に height(負なら逆向き)だけ、壁を傾けて押し出す。drafts[i] はループ i の
 * SmApiCreateTaperExtrude の角度(度。押し出す向きから測り、0 でまっすぐ、正は内へ、負は外へ傾く)。
 * ループごとに作り、holes[i] が 0 でなければ(穴)引き、0 なら足す。 */
int jc_extrude_taper(const JcPlane* plane, const JcSegment* segments, const uint32_t* loop_lens,
                     size_t loop_count, double height, const double* drafts, const int32_t* holes,
                     JcBrep** out);

/* 輪郭を軸まわりに angle ラジアン回す。輪郭のループごとに回して足し引きする。 */
int jc_revolve(const JcPlane* plane, const JcSegment* segments, const uint32_t* loop_lens,
               size_t loop_count, const double axis_origin[3], const double axis_dir[3],
               double angle, JcBrep** out);

int jc_brep_copy(const JcBrep* brep, JcBrep** out);
void jc_brep_free(JcBrep* brep);

/* op: 0 和、1 差、2 積。target をその場で書き換え、tool を消す。 */
int jc_boolean(JcBrep* target, JcBrep* tool, int32_t op);

/* 行優先の 4x4 行列(列ベクトルにかける約束、平行移動は 4 列目)をかける(その場で)。 */
int jc_transform(JcBrep* brep, const double m[16]);

/* シェル: brep は変えずに、*out へ殻を作る。open は取り除く面(jc_faces の添字)、offset は
 * 厚み(SMLib の向き。負で内側)。 */
// 形 brep の平らな面 faces(同じ平面)の縁をそのまま、面の外向きの法線へ from〜to だけ押し出した立体
// (面を選んで押し出す時、縁が直線と円弧でない面の道具。縁の曲線を写すので、もとの形の辺とずれない)
int jc_extrude_faces(const JcBrep* brep, const size_t* faces, size_t count, double from, double to, JcBrep** out);
int jc_shell(const JcBrep* brep, const size_t* open, size_t open_count, double offset, JcBrep** out);
/* jc_shell の SMLib の指定を選べる版。flags: 1 = 凸の辺を延ばして交わらせる、2 = 自己交差を扱う */
int jc_shell_with(const JcBrep* brep, const size_t* open, size_t open_count, double offset, int32_t flags,
                  JcBrep** out);

/* 丸めの面の削除: brep は変えずに、丸めの面 faces(jc_faces の添字)を消して両側の面を延ばし、
 * 鋭い辺に戻した形を *out に作る(SMLib の SmBrep::DeleteFilletFace。丸めでない面は断る)。 */
int jc_delete_fillet_faces(const JcBrep* brep, const size_t* faces, size_t count, JcBrep** out);

/* 曲面の網から閉じた立体を作る(スイープ・ロフト)。曲面は NURBS で、制御点は u ごとに v の並び
 * (points[(iu * count_v + iv) * 4 + 0..3] が x, y, z, 重み)、ノットは端を含む全部
 * (count + degree + 1 個)。面にはタグ tag を付ける。caps の 1 番目の印(1)なら u の最小の
 * 境目の線を、2 番目の印(2)なら最大の境目の線を、端の平らな面の輪郭に使う
 * (始まりの面のタグは start_tag、終わりは end_tag)。面を縫って閉じた立体にならなければ断る。 */
typedef struct JcPatch {
    uint32_t degree_u, degree_v;
    uint32_t count_u, count_v;
    const double* knots_u;
    const double* knots_v;
    const double* points;
    int64_t tag;
    int32_t caps;
} JcPatch;
int jc_patch_solid(const JcPatch* patches, size_t count, int64_t start_tag, int64_t end_tag, JcBrep** out);

/* STEP の面 1 枚(M56)。曲面は kind: 0 平面、1 円筒(r1 = 半径)、2 円すい(r1 = frame の原点の高さの
 * 半径、r2 = 半頂角のラジアン)、3 球(r1)、4 円環(r1 = 大きい半径、r2 = 小さい半径)、5 B スプライン
 * (patch。tag と caps は使わない)。frame は原点・x・y・z の軸の 12 個。輪は loop_sizes の数ずつの
 * curves(輪を回る向き、1 本目の輪が外)。points は 1 点だけの輪(円すいの頂点)。 */
typedef struct JcCurve {
    uint32_t degree, count;
    const double* knots; /* count + degree + 1 個 */
    const double* points; /* x, y, z, 重み を count 組 */
} JcCurve;
typedef struct JcStepFace {
    int32_t kind;
    double frame[12];
    double r1, r2, h; /* 円すいは底の半径・上の半径・高さ */
    JcPatch patch;
    int32_t same_sense;
    uint32_t loop_count;
    const uint32_t* loop_sizes;
    const JcCurve* curves;
    uint32_t point_count;
    const double* points;
    int64_t tag;
} JcStepFace;
/* 面を辺の曲線で切り抜いて縫い、閉じた立体にする。失敗した面の添字を *failed に書く(面でなければ -1)。 */
int jc_step_solid(const JcStepFace* faces, size_t count, double tolerance, int32_t* failed, JcBrep** out);

int jc_face_count(const JcBrep* brep, size_t* out);
/* 面を out[0..cap] に書く。cap は jc_face_count 以上にすること。
 * flags: JC_FACE_AREA なら面積を求める(無ければ 0)。point は、JC_FACE_INNER_POINT なら面の内側の点、
 * JC_FACE_INNER_POINT_CURVED なら曲面だけ内側の点、それ以外は面の外形の箱の中心
 * (安いが、面の上にあるとは限らない)。内側の点を求めるのは 1 面あたり 0.3〜1 ms かかる。 */
enum { JC_FACE_AREA = 1, JC_FACE_INNER_POINT = 2, JC_FACE_INNER_POINT_CURVED = 4 };
int jc_faces(const JcBrep* brep, int32_t flags, JcFace* out, size_t cap);
/* jc_faces の並びで、面ごとのタグを付け直す。 */
int jc_set_face_tags(JcBrep* brep, const int64_t* tags, size_t count);

/* 辺の一覧。*out は jc_free_buffer で消す。 */
int jc_edges(const JcBrep* brep, JcEdge** out, size_t* count);
void jc_free_buffer(void* p);
/* 面 face(jc_faces の添字)から点 p までの距離。 */
int jc_face_distance(const JcBrep* brep, size_t face, const double p[3], double* out);
/* 円の辺の一覧。*out は jc_free_buffer で消す。 */
int jc_arc_edges(const JcBrep* brep, JcArcEdge** out, size_t* count);

/* 2 つの面の境目の辺すべて(形を問わない)。辺は points[first .. first + count] の折れ線で、
 * 両端は辺の端の点。convex は出っ張った辺か。smooth は両側の面が辺の真ん中で接している(なめらかな
 * 継ぎ目)なら 1、角なら 0、面の法線を求められず分からなければ -1。out と points は jc_free_buffer で消す。 */
typedef struct JcAnyEdge {
    uint32_t f0, f1;
    int32_t convex;
    int32_t smooth;
    uint32_t first, count;
} JcAnyEdge;
int jc_all_edges(const JcBrep* brep, JcAnyEdge** out, size_t* count, double** points, size_t* point_count);

/* 辺を丸める(flat = 0)か面取りする(flat = 1。断面は直線)。辺は両側の面の添字と、辺の近くの点で指す。
 * size は丸めの半径(面取りでも、面に接する円の半径として渡す)。形状をその場で変える。
 * law は半径の決め方: 0 = 一定、1 = 弦の長さ(size が断面の両端の間の距離)、2 = 途中で変わる
 * (radii[2 * first ..] の count 組の (位置, 半径)。位置は start に近い辺の端を 0、反対の端を 1 とした割合)。
 * g2 = 1 なら断面を曲率までつなぐ(blend curve の断面)。weight はその断面のふくらみ(G2 の辺の最初の値を使う)。 */
typedef struct JcBlend {
    uint32_t f0, f1;
    double point[3];
    double size;
    int32_t flat;
    int32_t law;
    int32_t g2;
    double weight;
    double start[3];
    uint32_t first, count;
} JcBlend;
int jc_blend_edges(JcBrep* brep, const JcBlend* blends, size_t count, const double* radii);

int jc_mass(const JcBrep* brep, double relative_accuracy, JcMassProps* out);
/* 体積だけ(jc_mass の volume と同じ値。表面積と外形の箱を求めない分軽い) */
int jc_volume(const JcBrep* brep, double relative_accuracy, double* out);
int jc_bounds(const JcBrep* brep, double min[3], double max[3]);
/* 閉じた正しい立体か(多様体の立体)、つながった立体の数、中の空洞の数。 */
int jc_check(const JcBrep* brep, int32_t* manifold_solid, int64_t* solids, int64_t* voids);

/* chord は面からのずれの上限(mm)、angle_deg は隣の三角形の法線の角度の上限。 */
int jc_mesh(const JcBrep* brep, double chord, double angle_deg, JcMesh** out);
void jc_mesh_free(JcMesh* mesh);

/* STEP(AP214、mm)の文字列を書く。names[i] は立体の名前(NULL なら製品名から決める)。
 * *out は jc_free_buffer で消す。 */
int jc_export_step(const JcBrep* const* breps, const char* const* names, size_t count,
                   const char* product, char** out);

#ifdef __cplusplus
}
#endif

#endif
