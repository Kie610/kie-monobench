// SMLib(SM_API)を C の関数として見せる中継層。説明は jc_smlib.h。
// 形の判断(面の名前の決め方など)はここでは行わず、Rust 側に任せる。

#include "jc_smlib.h"

#include <SmApiBrep.h>
#include <SmApiCurves.h>
#include <SmPrimitiveCreation.h>
#include <SmApiGeneral.h>
#include <SmApiQueries.h>
#include <SmApiTrimmedSurfaces.h>
#include <SmCone.h>
#include <SmCylinder.h>
#include <SmPlane.h>
#include <SmSphere.h>
#include <SmTorus.h>
#include <SmTrimmingTools.h>
#include <SmStitch.h>
#include <SmAttribute.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmFilletExecutive.h>
#include <SmFaceuse.h>
#include <SmLine.h>
#include <SmPoly.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmVertex.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <stdexcept>
#include <memory>
#include <string>
#include <vector>

struct JcBrep; // 中身は SmBrep。型を分けるためだけの宣言

namespace
{

// 同じ曲面の上の 2 つの面の境の辺か。ブーリアンで閉じた曲面の面が分かれると(円柱の側面を貫く切り取り
// など)、元の継ぎ目の所が 2 つの面の境になり IsSeam では見分けられない。形の上に特別な所は無いので、
// 継ぎ目と同じく描かない
bool splits_one_surface(const SmEdge* e)
{
    SmTArray<SmFace*> fs;
    e->GetFaces(fs);
    if (fs.GetSize() != 2 || !fs[0] || !fs[1] || fs[0] == fs[1])
        return false;
    const SmSurface* a = fs[0]->GetSurface();
    const SmSurface* b = fs[1]->GetSurface();
    return a && b && (a == b || *a == *b);
}
// 面のタグの属性 ID。アプリ用の範囲(10000 以上)から選ぶ。
constexpr ULONG kTagAttribute = SM_AI_USER_1 + 610;

thread_local std::string g_error;

SmBrep* B(JcBrep* p) { return reinterpret_cast<SmBrep*>(p); }
const SmBrep* B(const JcBrep* p) { return reinterpret_cast<const SmBrep*>(p); }
JcBrep* J(SmBrep* p) { return reinterpret_cast<JcBrep*>(p); }

struct Failure
{
    std::string message;
};

void check(SmApiStatus st, const char* what)
{
    if (st != SM_SUCCESS)
        throw Failure{std::string(what) + " に失敗しました(SMLib の状態 " + std::to_string(st) + ")"};
}

void ensure_context()
{
    static bool ready = false;
    if (!ready)
    {
        SmApiCreateContext();
        ready = true;
    }
}

template <class F>
int guarded(F&& f)
{
    try
    {
        ensure_context();
        f();
        g_error.clear();
        return 0;
    }
    catch (const Failure& e)
    {
        g_error = e.message;
    }
    catch (const std::exception& e)
    {
        g_error = std::string("SMLib の中で例外が起きました: ") + e.what();
    }
    catch (...)
    {
        g_error = "SMLib の中で不明な例外が起きました";
    }
    return 1;
}

struct BrepOwner
{
    void operator()(SmBrep* b) const { delete b; }
};
using BrepPtr = std::unique_ptr<SmBrep, BrepOwner>;

SmVector3d vec(const double v[3]) { return SmVector3d(v[0], v[1], v[2]); }

void put(double out[3], const SmPoint3d& p)
{
    out[0] = p.x;
    out[1] = p.y;
    out[2] = p.z;
}

SmVector3d cross(const SmVector3d& a, const SmVector3d& b)
{
    return SmVector3d(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
}

double dot(const SmVector3d& a, const SmVector3d& b) { return a.x * b.x + a.y * b.y + a.z * b.z; }

SmVector3d unit(SmVector3d v)
{
    const double l = std::sqrt(dot(v, v));
    return l > 0 ? SmVector3d(v.x / l, v.y / l, v.z / l) : v;
}

constexpr double kPi = 3.14159265358979323846;

// 多角形 `pts`(平面の 2D、`flat` の向きから見た並び)を耳から切り落として三角形にする。
// 分割の部品が四角より大きな多角形のまま残ることがあり(シェルの後の、穴が多い平らな面など)、
// 扇形に切ると穴をまたいでしまうため。穴は橋でつないだ 1 本の輪として来る。並びの番号を返す。
std::vector<uint32_t> ear_clip(const std::vector<std::array<double, 2>>& pts)
{
    const auto cr = [&](size_t a, size_t b, size_t c) {
        return (pts[b][0] - pts[a][0]) * (pts[c][1] - pts[a][1]) - (pts[b][1] - pts[a][1]) * (pts[c][0] - pts[a][0]);
    };
    const auto same = [&](size_t a, size_t b) { return pts[a] == pts[b]; };
    std::vector<size_t> poly(pts.size());
    for (size_t i = 0; i < poly.size(); ++i)
        poly[i] = i;
    double area = 0;
    for (size_t i = 0; i < poly.size(); ++i)
        area += cr(0, i, (i + 1) % poly.size());
    if (area < 0)
        std::reverse(poly.begin(), poly.end());
    std::vector<uint32_t> out;
    size_t guard = 0;
    while (poly.size() > 3 && guard < poly.size() * 2)
    {
        const size_t n = poly.size();
        size_t cut = n;
        for (size_t i = 0; i < n && cut == n; ++i)
        {
            const size_t a = poly[(i + n - 1) % n], b = poly[i], c = poly[(i + 1) % n];
            if (cr(a, b, c) <= 1e-14)
                continue;
            bool blocked = false;
            for (size_t k : poly)
            {
                if (k == a || k == b || k == c || same(k, a) || same(k, b) || same(k, c))
                    continue;
                const double d1 = cr(a, b, k), d2 = cr(b, c, k), d3 = cr(c, a, k);
                if (!((d1 < 0 || d2 < 0 || d3 < 0) && (d1 > 0 || d2 > 0 || d3 > 0)))
                {
                    blocked = true;
                    break;
                }
            }
            if (!blocked)
                cut = i;
        }
        if (cut < n)
        {
            out.insert(out.end(), {uint32_t(poly[(cut + n - 1) % n]), uint32_t(poly[cut]), uint32_t(poly[(cut + 1) % n])});
            poly.erase(poly.begin() + long(cut));
            guard = 0;
        }
        else
        {
            // 耳が見つからない(自己交差など): 一直線の点を 1 つ捨てて続ける
            poly.erase(poly.begin());
            ++guard;
        }
    }
    if (poly.size() == 3 && cr(poly[0], poly[1], poly[2]) > 0)
        out.insert(out.end(), {uint32_t(poly[0]), uint32_t(poly[1]), uint32_t(poly[2])});
    // 輪の並びを逆にした時は、三角形も元の並びの向きに戻す
    if (area < 0)
        for (size_t t = 0; t + 2 < out.size(); t += 3)
            std::swap(out[t + 1], out[t + 2]);
    return out;
}

// 区間ごとの曲線(平面の 2D 座標、z = 0)。失敗したら作った曲線を消す。
struct Curves
{
    std::vector<SmCurve*> items;
    ~Curves()
    {
        for (SmCurve* c : items)
            delete c;
    }
    SmTArray<SmCurve*> array() const
    {
        SmTArray<SmCurve*> a;
        for (SmCurve* c : items)
            a.Add(c);
        return a;
    }
    // SMLib が引き取ったので、消さない
    void release() { items.clear(); }
};

void add_segment(Curves& out, const JcSegment& s)
{
    switch (s.kind)
    {
    case JC_SEG_LINE: {
        SmLine* line = nullptr;
        check(SmApiCreateLineSegment(SmPoint3d(s.a[0], s.a[1], 0), SmPoint3d(s.b[0], s.b[1], 0), line), "直線");
        out.items.push_back(line);
        break;
    }
    case JC_SEG_ARC: {
        double sweep = std::fmod(s.end - s.start, 2 * kPi);
        if (sweep < 0)
            sweep += 2 * kPi;
        if (sweep < 1e-12)
            sweep = 2 * kPi;
        // SmApiCreateArc は終わりの角度が 360° を超えると失敗する(状態 1007)ので、
        // 始まりを 1 周戻して終わりを 360° 以内にする(例: 330°〜390° → -30°〜30°)
        double a0 = std::fmod(s.start * 180.0 / kPi, 360.0);
        const double sweep_deg = sweep * 180.0 / kPi;
        if (a0 + sweep_deg > 360.0)
            a0 -= 360.0;
        SmBSplineCurve* arc = nullptr;
        check(SmApiCreateArc(SmPoint3d(s.center[0], s.center[1], 0), s.radius, a0, a0 + sweep_deg, arc),
              "円弧");
        out.items.push_back(arc);
        break;
    }
    case JC_SEG_CIRCLE: {
        SmCircle* c = nullptr;
        check(SmApiCreateCircle(SmPoint3d(s.center[0], s.center[1], 0), s.radius, c), "円");
        out.items.push_back(c);
        break;
    }
    default:
        throw Failure{"区間の種類が不正です"};
    }
}

// 平面の 2D 座標から空間への行列。SMLib の SmBrep::Transform は行ベクトルの約束で、
// 行 0〜2 が x・y・z 軸の行き先、行 3 が平行移動。z は法線方向で、offset だけずらす。
void plane_matrix(const JcPlane& p, double offset, double m[4][4])
{
    const SmVector3d x = vec(p.x_axis), y = vec(p.y_axis), n = unit(cross(x, y));
    const double rows[4][3] = {{x.x, x.y, x.z},
                               {y.x, y.y, y.z},
                               {n.x, n.y, n.z},
                               {p.origin[0] + n.x * offset, p.origin[1] + n.y * offset, p.origin[2] + n.z * offset}};
    for (int r = 0; r < 4; ++r)
    {
        for (int c = 0; c < 3; ++c)
            m[r][c] = rows[r][c];
        m[r][3] = r == 3 ? 1.0 : 0.0;
    }
}

SmAxis2Placement placement(const double m[4][4])
{
    return SmAxis2Placement(SmPoint3d(m[3][0], m[3][1], m[3][2]), SmVector3d(m[0][0], m[0][1], m[0][2]),
                            SmVector3d(m[1][0], m[1][1], m[1][2]));
}

// 平面の上の面(シート)を作る。ループはすべて同じ平面の上にある。
// 曲線を平面の位置へ動かしてから面にする(面にしてから SmBrep::Transform で動かすと、
// 円の押し出しが 10 倍ほど遅くなった。2026-10-13 に実測)。
BrepPtr planar_sheet(const JcPlane& plane, const JcSegment* segs, const uint32_t* lens, size_t loops, double offset)
{
    Curves curves;
    size_t k = 0;
    for (size_t i = 0; i < loops; ++i)
        for (uint32_t j = 0; j < lens[i]; ++j)
            add_segment(curves, segs[k++]);
    double m[4][4];
    plane_matrix(plane, offset, m);
    const SmAxis2Placement place = placement(m);
    for (SmCurve* c : curves.items)
        check(SmApiTransform(c, place), "輪郭の移動");
    SmBrep* sheet = nullptr;
    check(SmApiCreatePlanarFaces(curves.array(), sheet), "輪郭から面を作る処理");
    curves.release();
    return BrepPtr(sheet);
}

// 面の外向きの向き: 面の上側の面使用が無限の領域(外)に接していれば、曲面の法線のまま。
double outward_sign(const SmFace* face)
{
    SmBrep* brep = face->GetBrep();
    SmFaceuse* up = face->GetUpwardFaceuse();
    if (brep && up && up->GetShell() && brep->GetInfiniteRegion() != up->GetShell()->GetRegion())
        return -1.0;
    return 1.0;
}

// 有理曲面の制御点が同じ平面にあれば、曲面全体もその平面に載る。
// 点へ縮む端を持つ三角の曲面では、外形の箱による平面の判定だけでは見落とすことがある。
bool planar_control_net(const SmSurface* surface, SmPoint3d* point)
{
    if (!surface->IsKindOf(SmBSplineSurface_TYPE))
        return false;
    const auto* spline = static_cast<const SmBSplineSurface*>(surface);
    ULONG nu = spline->GetNumberControlPoints(SM_SP_U), nv = spline->GetNumberControlPoints(SM_SP_V);
    // 境の列が点へ縮む曲面に限る。通常の輪郭どうしのロフトの判定は変えない。
    const auto collapsed = [&](bool fixed_u, ULONG index) {
        SmPoint3d anchor;
        double weight = 0;
        if (spline->GetControlPoint(SM_CP_NON_RATIONAL, fixed_u ? index : 0, fixed_u ? 0 : index, anchor, weight) != SM_SUCCESS)
            return false;
        const ULONG count = fixed_u ? nv : nu;
        for (ULONG i = 1; i < count; ++i)
        {
            SmPoint3d p;
            if (spline->GetControlPoint(SM_CP_NON_RATIONAL, fixed_u ? index : i, fixed_u ? i : index, p, weight) != SM_SUCCESS)
                return false;
            const SmVector3d d(p.x - anchor.x, p.y - anchor.y, p.z - anchor.z);
            if (dot(d, d) > 1e-18)
                return false;
        }
        return true;
    };
    if (nu < 2 || nv < 2 || !(collapsed(true, 0) || collapsed(true, nu - 1) || collapsed(false, 0) || collapsed(false, nv - 1)))
        return false;
    SmTArray<SmPoint3d> points;
    SmTArray<double> weights;
    if (spline->GetControlPointNet(nu, nv, points, weights) != SM_SUCCESS || points.GetSize() < 3)
        return false;
    const SmPoint3d origin = points[0];
    const auto offset = [&](const SmPoint3d& p) { return SmVector3d(p.x - origin.x, p.y - origin.y, p.z - origin.z); };
    SmVector3d along(0, 0, 0), normal(0, 0, 0);
    double longest = 0;
    for (ULONG i = 0; i < points.GetSize(); ++i)
    {
        const auto& p = points[i];
        if (!std::isfinite(p.x) || !std::isfinite(p.y) || !std::isfinite(p.z))
            return false;
        const SmVector3d d = offset(p);
        if (dot(d, d) > longest)
        {
            longest = dot(d, d);
            along = d;
        }
    }
    for (ULONG i = 0; i < weights.GetSize(); ++i)
        if (!std::isfinite(weights[i]) || weights[i] <= 0)
            return false;
    if (longest < 1e-18)
        return false;
    along = unit(along);
    double widest = 0;
    for (ULONG i = 0; i < points.GetSize(); ++i)
    {
        const SmVector3d n = cross(along, offset(points[i]));
        if (dot(n, n) > widest)
        {
            widest = dot(n, n);
            normal = n;
        }
    }
    if (widest < 1e-18)
        return false;
    normal = unit(normal);
    for (ULONG i = 0; i < points.GetSize(); ++i)
        if (std::fabs(dot(offset(points[i]), normal)) > 1e-9)
            return false;
    if (point)
        *point = origin;
    return true;
}

bool planar_normal(const SmFace* face, SmVector3d& normal, SmPoint3d* point = nullptr)
{
    const SmSurface* s = face->GetSurface();
    if (!s || (!s->IsPlanar(1e-9, point) && !planar_control_net(s, point)))
        return false;
    SmVector3d n;
    if (s->EvaluateNormal(face->GetUVDomain().GetMid(), TRUE, TRUE, n) != SM_SUCCESS)
        return false;
    n = unit(n);
    const double sg = outward_sign(face);
    normal = SmVector3d(n.x * sg, n.y * sg, n.z * sg);
    return true;
}

SmTArray<SmFace*> faces_of(const SmBrep* b)
{
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    return faces;
}

long face_index(const SmTArray<SmFace*>& faces, const SmFace* f)
{
    for (ULONG i = 0; i < faces.GetSize(); ++i)
        if (faces[i] == f)
            return long(i);
    return -1;
}
} // namespace

// 別のファイル(jc_step.cpp)の処理を、同じ後始末(例外を文字列にして返す)で包む。
// std::runtime_error の文はそのまま理由にする。
int jc_guarded_call(void (*body)(void*), void* data)
{
    return guarded([&] {
        try
        {
            body(data);
        }
        catch (const std::runtime_error& e)
        {
            throw Failure{e.what()};
        }
    });
}

extern "C" {

const char* jc_last_error(void) { return g_error.c_str(); }

int jc_planar_sheet(const JcPlane* plane, const JcSegment* segments, const uint32_t* lens, size_t loops, JcBrep** out)
{
    return guarded([&] { *out = J(planar_sheet(*plane, segments, lens, loops, 0).release()); });
}

int jc_is_sheet(const JcBrep* brep, int32_t* out)
{
    return guarded([&] { *out = B(brep)->IsSheet() ? 1 : 0; });
}

// 区間の曲線は境界専用の複製にする。ボディの辺は曲線の有効な区間を切り出す。
static SmBSplineCurve* boundary_curve(const JcBoundary& s)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    SmBSplineCurve* curve = nullptr;
    const SmPoint3d a(s.points[0], s.points[1], s.points[2]);
    if (s.kind == 0)
        check(SmBSplineCurve::CreateLineSegment(ctx, 3, a, SmPoint3d(s.points[3], s.points[4], s.points[5]), curve), "縁の直線");
    else if (s.kind == 1)
    {
        SmTArray<SmPoint3d> pts;
        SmTArray<SmVector3d> derivs;
        for (uint32_t i = 0; i < s.count; ++i)
            pts.Add(SmPoint3d(s.points[i*3], s.points[i*3+1], s.points[i*3+2]));
        check(SmBSplineCurve::CreateInterpolatingCurve(ctx, SM_CP_CHORDLENGTH, 3, std::min(3u, s.count-1), pts, derivs, nullptr, TRUE, curve), "縁の補間曲線");
    }
    else if (s.kind == 2)
    {
        // 平面の円弧を作り、始まりの点と軸が決める平面へ動かす。
        const SmPoint3d center(s.center[0], s.center[1], s.center[2]);
        SmVector3d x = unit(SmVector3d(a.x-center.x, a.y-center.y, a.z-center.z));
        SmVector3d n = vec(s.axis);
        if (s.sweep < 0) n = SmVector3d(-n.x, -n.y, -n.z);
        check(SmApiCreateArc(SmPoint3d(0,0,0), a.DistanceBetween(center), 0, std::fabs(s.sweep)*180/kPi, curve), "縁の円弧");
        check(SmApiTransform(curve, SmAxis2Placement(center, x, cross(unit(n), x))), "縁の円弧の位置");
    }
    else if (s.kind == 3)
    {
        const SmTArray<SmFace*> fs = faces_of(B(s.brep));
        SmTArray<SmEdge*> es;
        B(s.brep)->GetEdges(es);
        SmEdge* found = nullptr;
        double best = 1e100;
        for (ULONG i = 0; i < es.GetSize(); ++i)
        {
            SmTArray<SmFace*> ef;
            es[i]->GetFaces(ef);
            if (ef.GetSize()!=2 && ef.GetSize()!=1) continue;
            const long f0 = face_index(fs, ef[0]);
            const uint32_t f1 = ef.GetSize()==1 ? UINT32_MAX : uint32_t(face_index(fs, ef[1]));
            if (!((f0==s.f0 && f1==s.f1)||(f0==s.f1 && f1==s.f0))) continue;
            SmPoint3d q; double d=0, parameter=0;
            const SmPoint3d pick(s.center[0], s.center[1], s.center[2]);
            if (SmApiEdgeClosestPoint(es[i], pick, q, parameter, d)==SM_SUCCESS && d<best) { best=d; found=es[i]; }
        }
        if (!found || best>1e-4) throw Failure{"選んだボディの辺が見つかりません。縁を選び直してください"};
        double lo=0, hi=0;
        check(SmApiEdgeParameterRange(found, lo, hi), "辺の範囲");
        SmTArray<double> breaks; breaks.Add(lo); breaks.Add(hi);
        double gap=0;
        check(found->GetCurve()->ApproximateCurve(ctx, breaks, 1e-7, gap, curve, FALSE, TRUE, TRUE), "辺の曲線の複製");
        SmExtent1d interval(lo, hi);
        check(curve->Trim(interval), "辺の曲線の区間");
        SmPoint3d start, end;
        check(curve->EvaluatePoint(interval.GetMin(), start), "辺の始まり");
        check(curve->EvaluatePoint(interval.GetMax(), end), "辺の終わり");
        if (a.DistanceBetween(end) < a.DistanceBetween(start)) { SmExtent1d reversed; check(curve->ReverseParameterization(interval, reversed), "辺の向き"); }
    }
    return curve;
}

int jc_patch_sheet(const JcBoundary* boundary, size_t count, int32_t is_planar, JcBrep** out)
{
    return guarded([&] {
        const SmContext& ctx = *SmApiGetOrCreateContext();
        Curves owned;
        SmTArray<SmBSplineCurve*> curves, derivs;
        for (size_t i=0; i<count; ++i) {
            SmBSplineCurve* c=boundary_curve(boundary[i]); owned.items.push_back(c); curves.Add(c);
        }
        SmBrep* planar=nullptr;
        if (is_planar && SmApiCreatePlanarFaces(owned.array(), planar)==SM_SUCCESS && planar) {
            owned.release(); *out=J(planar); return;
        }
        delete planar;
        BrepPtr b(new (ctx) SmBrep());
        b->m_bEditingEnabled=TRUE;
        SmTemporaryChangeValue<SmBoolean> doing(SM_CONST_CAST(SmContext*, &ctx)->GetDoingBooleanRef(), TRUE);
        if (count==4) {
            // Coons の口は左・右・下・上の順で、向かい合う曲線を同じ向きに渡す。
            SmExtent1d reversed;
            check(curves[2]->ReverseParameterization(curves[2]->GetNaturalInterval(), reversed), "上の縁の向き");
            check(curves[3]->ReverseParameterization(curves[3]->GetNaturalInterval(), reversed), "左の縁の向き");
            SmTArray<SmBSplineCurve*> sides;
            sides.Add(curves[3]); sides.Add(curves[1]); sides.Add(curves[0]); sides.Add(curves[2]);
            SmBSplineSurface* surface=nullptr;
            check(SmBSplineSurface::CreateCoonsPatch(ctx, 1e-4, TRUE, sides, derivs, surface), "縁から曲面を張る処理");
            SmTArray<SmFace*> faces;
            check(b->CreateFacesFromSurface(surface, surface->GetNaturalUVDomain(), SM_CT_C0, faces), "曲面から面を作る処理");
        } else {
            SmTArray<SmSurface*> surfaces;
            // 接線を指定しない時も、境界と同じ数の空欄を渡す。
            for (size_t i=0;i<count;++i) derivs.Add(nullptr);
            const SmStatus st=SmBSplineSurface::CreateNSidedPatch(ctx, curves, derivs, 1.0, surfaces);
            if (st!=SM_SUCCESS) {
                for (ULONG i=0;i<surfaces.GetSize();++i) delete surfaces[i];
                if (count>=5) throw Failure{"縁が 5 本以上の面は、まだ張れません。縁を 4 本までにしてください"};
                check(st, "縁から曲面を張る処理");
            }
            for (ULONG i=0;i<surfaces.GetSize();++i) {
                SmTArray<SmFace*> faces;
                check(b->CreateFacesFromSurface(surfaces[i], surfaces[i]->GetNaturalUVDomain(), SM_CT_C0, faces), "曲面から面を作る処理");
            }
            SmBoolean wellFormed=FALSE;
            ULONG stitched=0; double vertexGap=0, edgeGap=0;
            check(SmApiStitchIntoShell(b.get(), wellFormed, 1e-4, stitched, vertexGap, edgeGap), "曲面の縫い合わせ");
        }
        if (!b->IsSheet()) throw Failure{"張った面が面のボディになりませんでした。縁を描き直してください"};
        *out=J(b.release());
    });
}

int jc_extrude(const JcPlane* plane, const JcSegment* segments, const uint32_t* loop_lens, size_t loop_count,
               double from, double to, JcBrep** out)
{
    return guarded([&] {
        if (!(to > from))
            throw Failure{"押し出しの距離は 0 より大きくしてください"};
        BrepPtr b = planar_sheet(*plane, segments, loop_lens, loop_count, from);
        const SmVector3d n = unit(cross(vec(plane->x_axis), vec(plane->y_axis)));
        check(SmApiNonManifoldSweep(b.get(), nullptr, n, to - from, TRUE, FALSE, nullptr, nullptr, nullptr),
              "押し出し");
        *out = J(b.release());
    });
}

int jc_extrude_taper(const JcPlane* plane, const JcSegment* segments, const uint32_t* loop_lens, size_t loop_count,
                     double height, const double* drafts, const int32_t* holes, JcBrep** out)
{
    return guarded([&] {
        if (!(std::fabs(height) > 1e-12))
            throw Failure{"押し出しの距離は 0 より大きくしてください"};
        double m[4][4];
        plane_matrix(*plane, 0.0, m);
        // SmApiCreateTaperExtrude は曲線の平面の法線へ押し出す。輪郭の平面の法線と逆に向く
        // ことがあるので、出来た形の高さを見て、逆なら反対の高さで作り直す
        const SmVector3d n = unit(cross(vec(plane->x_axis), vec(plane->y_axis)));
        const SmPoint3d o(plane->origin[0], plane->origin[1], plane->origin[2]);
        BrepPtr result;
        size_t k = 0;
        for (size_t i = 0; i < loop_count; ++i)
        {
            const size_t first = k;
            k += loop_lens[i];
            BrepPtr p;
            for (double h : {height, -height})
            {
                Curves curves;
                for (size_t j = first; j < k; ++j)
                    add_segment(curves, segments[j]);
                const SmAxis2Placement place = placement(m);
                for (SmCurve* c : curves.items)
                    check(SmApiTransform(c, place), "輪郭の移動");
                SmBrep* piece = nullptr;
                check(SmApiCreateTaperExtrude(curves.array(), h, drafts[i], 3, FALSE, piece), "テーパーの押し出し");
                BrepPtr b(piece);
                SmPoint3d lo, hi;
                check(SmApiBrepBoundingBox(b.get(), FALSE, lo, hi), "外形の箱");
                // 箱の 8 つの角を法線へ映し、輪郭の平面から height の側へ伸びたかを見る
                double top = -1e300, bottom = 1e300;
                for (int c = 0; c < 8; ++c)
                {
                    const SmPoint3d q((c & 1) ? hi.x : lo.x, (c & 2) ? hi.y : lo.y, (c & 4) ? hi.z : lo.z);
                    const double d = (q.x - o.x) * n.x + (q.y - o.y) * n.y + (q.z - o.z) * n.z;
                    top = std::max(top, d);
                    bottom = std::min(bottom, d);
                }
                const double reach = height > 0 ? top : -bottom;
                if (reach > std::fabs(height) * 0.5)
                {
                    p = std::move(b);
                    break;
                }
            }
            if (!p)
                throw Failure{"テーパーの押し出しの向きを決められません"};
            if (!result)
            {
                result = std::move(p);
                continue;
            }
            // 穴は上下の面が外周の形と同じ平面に重なるので、位置関係でなく呼び出し側の印で決める
            SmBrep* r = nullptr;
            const SmApiStatus st = holes[i] ? SmApiBooleanDifference(result.get(), p.get(), r)
                                            : SmApiBooleanUnion(result.get(), p.get(), r);
            if (st == SM_SUCCESS)
                p.release();
            check(st, "テーパーの形の足し引き");
        }
        if (!result)
            throw Failure{"閉じた輪郭がありません"};
        *out = J(result.release());
    });
}

int jc_revolve(const JcPlane* plane, const JcSegment* segments, const uint32_t* loop_lens, size_t loop_count,
               const double axis_origin[3], const double axis_dir[3], double angle, JcBrep** out)
{
    return guarded([&] {
        double m[4][4];
        plane_matrix(*plane, 0.0, m);
        const double deg = angle >= 2 * kPi - 1e-9 ? 360.0 : angle * 180.0 / kPi;
        BrepPtr result;
        size_t k = 0;
        for (size_t i = 0; i < loop_count; ++i)
        {
            Curves curves;
            for (uint32_t j = 0; j < loop_lens[i]; ++j)
                add_segment(curves, segments[k++]);
            const SmAxis2Placement place = placement(m);
            for (SmCurve* c : curves.items)
                check(SmApiTransform(c, place), "輪郭の移動");
            SmBrep* piece = nullptr;
            check(SmApiCreateRotationalSweep(curves.array(), SmPoint3d(axis_origin[0], axis_origin[1], axis_origin[2]),
                                             vec(axis_dir), deg, TRUE, piece),
                  "回転");
            BrepPtr p(piece);
            if (!result)
            {
                result = std::move(p);
                continue;
            }
            // 2 つ目以降のループ: 最初のループの内側なら穴、外側なら別の立体として足す
            int rel = 0;
            double dist = 0;
            check(SmApiBrepRelationship(result.get(), p.get(), rel, dist), "回転の形の位置関係");
            SmBrep* r = nullptr;
            const SmApiStatus st = rel == 3 ? SmApiBooleanDifference(result.get(), p.get(), r)
                                            : SmApiBooleanUnion(result.get(), p.get(), r);
            if (st == SM_SUCCESS)
                p.release();
            check(st, "回転の形の足し引き");
        }
        if (!result)
            throw Failure{"閉じた輪郭がありません"};
        *out = J(result.release());
    });
}

// 端を含むノットの全部を、重ならない値と重なりの数にする。
static void unique_knots(const double* knots, size_t n, SmTArray<double>& values, SmTArray<ULONG>& mults)
{
    for (size_t i = 0; i < n; ++i)
    {
        const ULONG last = values.GetSize();
        if (last > 0 && std::fabs(knots[i] - values[last - 1]) < 1e-12)
            mults[last - 1] += 1;
        else
        {
            values.Add(knots[i]);
            mults.Add(1);
        }
    }
}

int jc_patch_solid(const JcPatch* patches, size_t count, int64_t start_tag, int64_t end_tag, JcBrep** out)
{
    return guarded([&] {
        const SmContext& ctx = *SmApiGetOrCreateContext();
        BrepPtr brep(new (ctx) SmBrep());
        brep->m_bEditingEnabled = TRUE;
        SmTemporaryChangeValue<SmBoolean> doing(SM_CONST_CAST(SmContext*, &ctx)->GetDoingBooleanRef(), TRUE);
        const auto tag_face = [&](SmFace* f, int64_t tag) {
            if (SmAttribute* old = f->FindAttribute(kTagAttribute))
                f->RemoveAttribute(old);
            f->AddAttribute(new (ctx) SmLongAttribute(kTagAttribute, long(tag), SM_AB_COPY));
        };
        SmTArray<SmCurve*> starts, ends;
        for (size_t i = 0; i < count; ++i)
        {
            const JcPatch& p = patches[i];
            SmTArray<double> uk, vk;
            SmTArray<ULONG> um, vm;
            unique_knots(p.knots_u, p.count_u + p.degree_u + 1, uk, um);
            unique_knots(p.knots_v, p.count_v + p.degree_v + 1, vk, vm);
            SmTArray<SmPoint3d> pts;
            SmTArray<double> weights;
            for (size_t k = 0; k < size_t(p.count_u) * p.count_v; ++k)
            {
                const double* q = p.points + k * 4;
                pts.Add(SmPoint3d(q[0], q[1], q[2]));
                weights.Add(q[3]);
            }
            SmBSplineSurface* srf = nullptr;
            check(SmBSplineSurface::CreateCanonical(ctx, p.degree_u, p.degree_v, pts, SM_SF_UNSPECIFIED, um, vm, uk, vk,
                                                    SM_KT_UNSPECIFIED, &weights, nullptr, srf),
                  "曲面");
            const SmExtent2d dom = srf->GetNaturalUVDomain();
            for (int end = 0; end < 2; ++end)
            {
                if (!(p.caps & (1 << end)))
                    continue;
                SmBSplineCurve* c = nullptr;
                check(srf->CreateIsoParametricCurve(ctx, SM_SP_U, end ? dom.GetUMax() : dom.GetUMin(), 0.0, c),
                      "端の線");
                (end ? ends : starts).Add(c);
            }
            SmTArray<SmFace*> faces;
            check(brep->CreateFacesFromSurface(srf, dom, SM_CT_G1, faces), "曲面の面");
            for (ULONG f = 0; f < faces.GetSize(); ++f)
                tag_face(faces[f], p.tag);
        }
        const double tol = brep->GetTolerance();
        for (int end = 0; end < 2; ++end)
        {
            SmTArray<SmCurve*>& curves = end ? ends : starts;
            if (curves.GetSize() == 0)
                continue;
            SmTArray<SmFace*> faces;
            const SmStatus st = brep->CreatePlanarFacesWith3DCurves(brep->GetInfiniteRegion(), curves, tol, faces);
            if (st != SM_SUCCESS)
            {
                for (ULONG k = 0; k < curves.GetSize(); ++k)
                    delete curves[k];
                check(SmApiStatus(st), end ? "終わりの端の面" : "始まりの端の面");
            }
            for (ULONG f = 0; f < faces.GetSize(); ++f)
                tag_face(faces[f], end ? end_tag : start_tag);
        }
        SmBoolean solid = FALSE;
        ULONG stitched = 0;
        double vgap = 0, egap = 0;
        check(SmApiStitchIntoSolid(brep.get(), solid, stitched, vgap, egap), "面を縫う処理");
        if (!solid)
            throw Failure{"面を縫っても閉じた立体になりませんでした(隙間 " + std::to_string(std::max(vgap, egap)) + ")"};
        *out = J(brep.release());
    });
}

static SmBSplineSurface* patch_surface(const SmContext& ctx, const JcPatch& p)
{
    SmTArray<double> uk, vk;
    SmTArray<ULONG> um, vm;
    unique_knots(p.knots_u, p.count_u + p.degree_u + 1, uk, um);
    unique_knots(p.knots_v, p.count_v + p.degree_v + 1, vk, vm);
    SmTArray<SmPoint3d> pts;
    SmTArray<double> weights;
    for (size_t k = 0; k < size_t(p.count_u) * p.count_v; ++k)
    {
        const double* q = p.points + k * 4;
        pts.Add(SmPoint3d(q[0], q[1], q[2]));
        weights.Add(q[3]);
    }
    SmBSplineSurface* srf = nullptr;
    check(SmBSplineSurface::CreateCanonical(ctx, p.degree_u, p.degree_v, pts, SM_SF_UNSPECIFIED, um, vm, uk, vk,
                                            SM_KT_UNSPECIFIED, &weights, nullptr, srf),
          "曲面");
    return srf;
}

static SmBSplineCurve* step_curve(const SmContext& ctx, const JcCurve& c)
{
    SmTArray<double> knots;
    SmTArray<ULONG> mults;
    unique_knots(c.knots, c.count + c.degree + 1, knots, mults);
    SmTArray<SmPoint3d> pts;
    SmTArray<double> weights;
    for (uint32_t k = 0; k < c.count; ++k)
    {
        const double* q = c.points + size_t(k) * 4;
        pts.Add(SmPoint3d(q[0], q[1], q[2]));
        weights.Add(q[3]);
    }
    SmBSplineCurve* curve = nullptr;
    check(SmBSplineCurve::CreateCanonical(ctx, 3, c.degree, pts, SM_CF_UNSPECIFIED, mults, knots, SM_KT_UNSPECIFIED,
                                          &weights, nullptr, curve),
          "辺の曲線");
    return curve;
}

static SmSurface* step_surface(const SmContext& ctx, const JcStepFace& f)
{
    const double* m = f.frame;
    const SmAxis2Placement ax(SmPoint3d(m[0], m[1], m[2]), SmVector3d(m[3], m[4], m[5]), SmVector3d(m[6], m[7], m[8]));
    switch (f.kind)
    {
    case 0: {
        SmPlane* s = nullptr;
        check(SmPlane::CreateCanonical(ctx, ax, s), "平面");
        return s;
    }
    case 1: {
        SmCylinder* s = nullptr;
        check(SmCylinder::CreateCanonical(ctx, ax, f.r1, s), "円筒");
        return s;
    }
    case 2: {
        SmCone* s = nullptr;
        check(SmCone::CreateCanonical(ctx, ax, f.r1, f.r2, f.h, s), "円すい");
        return s;
    }
    case 3: {
        SmSphere* s = nullptr;
        check(SmSphere::CreateCanonical(ctx, ax, f.r1, s), "球");
        return s;
    }
    case 4: {
        SmTorus* s = nullptr;
        check(SmTorus::CreateCanonical(ctx, ax, f.r1, f.r2, s), "円環");
        return s;
    }
    case 5:
        return patch_surface(ctx, f.patch);
    default:
        throw Failure{"知らない曲面"};
    }
}

// 曲面と境の曲線から面を作る。輪の解釈は呼ぶ側が宣言する。
static void trimmed_face(const SmContext& ctx, SmBrep* brep, const JcStepFace& f, double tolerance, bool uv, bool whole)
{
    SmSurface* srf = step_surface(ctx, f);
    SmTArray<ULONG> loops;
    SmTArray<SmCurve*> curves;
    SmTArray<SmOrientType> orients;
    size_t at = 0;
    for (uint32_t l = 0; l < f.loop_count; ++l)
    {
        loops.Add(f.loop_sizes[l]);
        for (uint32_t c = 0; c < f.loop_sizes[l]; ++c)
        {
            curves.Add(step_curve(ctx, f.curves[at++]));
            orients.Add(SM_OT_SAME);
        }
    }
    SmTArray<SmPoint3d> points;
    for (uint32_t k = 0; k < f.point_count; ++k)
        points.Add(SmPoint3d(f.points[k * 3], f.points[k * 3 + 1], f.points[k * 3 + 2]));
    SmTArray<SmFace*> made;
    if (whole && f.loop_count == 0 && f.point_count == 0 && (f.kind == 3 || f.kind == 4))
    {
        check(SmApiStatus(brep->CreateFacesFromSurface(srf, srf->GetNaturalUVDomain(), SM_CT_C0, made)), "閉じた曲面から面を作る処理");
        if (!f.same_sense)
            for (ULONG i = 0; i < made.GetSize(); ++i)
                check(SmApiStatus(made[i]->FlipFaceOrientation()), "面の向き");
    }
    else if (uv)
    {
        SmTArray<SmBSplineCurve*> parameterCurves;
        for (ULONG i = 0; i < curves.GetSize(); ++i)
            parameterCurves.Add(static_cast<SmBSplineCurve*>(curves[i]));
        check(SmApiStatus(SmTrimmingTools::TrimSurfaceWithParameterSpaceCurves(
                  ctx, brep->GetInfiniteRegion(), tolerance, loops, parameterCurves, orients, points,
                  srf, srf->GetNaturalUVDomain(), f.same_sense ? SM_OT_SAME : SM_OT_OPPOSITE, TRUE, FALSE)),
              "パラメータの境で面の切り抜き");
        brep->GetFaces(made);
    }
    else
    {
        check(SmApiStatus(SmTrimmingTools::TrimSurfaceWithModelSpaceCurves(
                  ctx, brep->GetInfiniteRegion(), tolerance, loops, curves, orients, points, srf,
                  srf->GetNaturalUVDomain(), f.same_sense ? SM_OT_SAME : SM_OT_OPPOSITE, TRUE, TRUE, TRUE,
                  &made)),
              "面の切り抜き");
    }
    if (made.GetSize() == 0)
        throw Failure{"面の切り抜き: 面ができませんでした"};
    for (ULONG k = 0; k < made.GetSize(); ++k)
    {
        // 面の範囲を切り抜いた輪の範囲にする(平面や円筒の曲面そのものは無限に広いので、
        // そのままだと面の内側の点を探す処理などが面を見つけられない)
        SmExtent2d dom;
        if (made[k]->CalculateUVDomainFromUVTrimCurves(dom, TRUE) == SM_SUCCESS)
            made[k]->SetUVDomain(dom);
        if (SmAttribute* old = made[k]->FindAttribute(kTagAttribute))
            made[k]->RemoveAttribute(old);
        made[k]->AddAttribute(new (ctx) SmLongAttribute(kTagAttribute, long(f.tag), SM_AB_COPY));
    }
}

int jc_step_solid(const JcStepFace* faces, size_t count, double tolerance, int32_t* failed, JcBrep** out)
{
    *failed = -1;
    return guarded([&] {
        const SmContext& ctx = *SmApiGetOrCreateContext();
        BrepPtr brep(new (ctx) SmBrep());
        brep->m_bEditingEnabled = TRUE;
        SmTemporaryChangeValue<SmBoolean> doing(SM_CONST_CAST(SmContext*, &ctx)->GetDoingBooleanRef(), TRUE);
        for (size_t i = 0; i < count; ++i)
        {
            *failed = int32_t(i);
            const JcStepFace& f = faces[i];
            trimmed_face(ctx, brep.get(), f, tolerance, false, false);
        }
        *failed = -1;
        // 縫う。SmApiStitchIntoSolid と同じく許容差を倍にしながら縫うが、辺を頂点で分ける
        // (円筒は継ぎ目で半分ずつの面に分かれ、平面の側は 1 周の円の辺のままなので、
        // 分けないと辺の組が合わない)。
        SmExtent3d box;
        check(SmApiStatus(brep->CalculateBoundingBox(box, TRUE)), "大きさの計算");
        const double size = box.GetSize().Length();
        double tol = brep->GetTolerance() / 10.0;
        ULONG lamina = 1;
        double vgap = 0, egap = 0;
        while (tol < size / 200.0)
        {
            SmStitchCallback cb;
            SmStitch st(cb, tol, TRUE, FALSE);
            st.m_bFastEdgeCompare = TRUE;
            st.m_bSplitEdgesWithVertices = TRUE;
            st.m_bMakingManifoldSolid = TRUE;
            ULONG stitched = 0;
            double v = 0, e = 0;
            check(SmApiStatus(st.DoStitching(brep.get(), NULL, NULL, stitched, lamina, v, e)), "面を縫う処理");
            vgap = std::max(vgap, v);
            egap = std::max(egap, e);
            if (lamina == 0)
                break;
            tol *= 2.0;
        }
        SmTArray<SmRegion*> regions;
        brep->GetRegions(regions);
        const bool solid = lamina == 0 && regions.GetSize() > 1;
        if (!solid)
            throw Failure{"面を縫っても閉じた立体になりませんでした(隙間 " + std::to_string(std::max(vgap, egap)) + ")"};
        *out = J(brep.release());
    });
}

int jc_iges_face(const JcStepFace* face, int32_t uv, double tolerance, JcBrep** out)
{
    return guarded([&] {
        const SmContext& ctx = *SmApiGetOrCreateContext();
        BrepPtr brep(new (ctx) SmBrep());
        brep->m_bEditingEnabled = TRUE;
        SmTemporaryChangeValue<SmBoolean> doing(SM_CONST_CAST(SmContext*, &ctx)->GetDoingBooleanRef(), TRUE);
        trimmed_face(ctx, brep.get(), *face, tolerance, uv != 0, true);
        *out = J(brep.release());
    });
}

int jc_iges_stitch(const JcBrep* const* faces, size_t count, double tolerance, int32_t mode, JcBrep** out)
{
    return guarded([&] {
        const SmContext& ctx = *SmApiGetOrCreateContext();
        BrepPtr brep(new (ctx) SmBrep());
        brep->m_bEditingEnabled = TRUE;
        for (size_t i = 0; i < count; ++i)
            check(SmApiStatus(brep->MergeBrep(*B(faces[i]))), "面を集める処理");
        if (mode >= 0)
        {
            SmStitchCallback cb;
            SmStitch st(cb, tolerance, TRUE, FALSE);
            st.m_bFastEdgeCompare = TRUE;
            st.m_bSplitEdgesWithVertices = TRUE;
            st.m_bMakingManifoldSolid = mode ? TRUE : FALSE;
            ULONG stitched = 0, lamina = 0;
            double vertexGap = 0, edgeGap = 0;
            check(SmApiStatus(st.DoStitching(brep.get(), NULL, NULL, stitched, lamina, vertexGap, edgeGap)), "面を縫う処理");
        }
        *out = J(brep.release());
    });
}

int jc_brep_copy(const JcBrep* brep, JcBrep** out)
{
    return guarded([&] {
        SmBrep* c = nullptr;
        check(SmApiBrepCopy(B(brep), c), "形状の複製");
        *out = J(c);
    });
}

void jc_brep_free(JcBrep* brep) { delete B(brep); }

int jc_boolean(JcBrep* target, JcBrep* tool, int32_t op)
{
    BrepPtr owned_tool(B(tool));
    return guarded([&] {
        const SmBooleanOperationType t = op == 0 ? SM_BO_UNION : op == 1 ? SM_BO_DIFFERENCE : SM_BO_INTERSECTION;
        SmBrep* r = nullptr;
        const SmApiStatus st = SmApiBoolean(B(target), owned_tool.get(), t, r);
        if (st == SM_SUCCESS)
            owned_tool.release(); // 成功したら SMLib が消している
        check(st, "ブーリアン");
    });
}

int jc_transform(JcBrep* brep, const double m[16])
{
    return guarded([&] {
        // 渡されるのは列ベクトルの約束(平行移動が 4 列目)。SMLib は行ベクトルの約束なので転置する
        double t[4][4];
        for (int r = 0; r < 4; ++r)
            for (int c = 0; c < 4; ++c)
                t[r][c] = m[c * 4 + r];
        // 回転と平行移動だけなら、拡大の指定なしの SmAxis2Placement で動かす
        // (4x4 の版は拡大率 1 を指定して呼ぶ。曲線では、その後の処理が遅くなった)
        const SmVector3d x(t[0][0], t[0][1], t[0][2]), y(t[1][0], t[1][1], t[1][2]), z(t[2][0], t[2][1], t[2][2]);
        // z の長さも見る(x・y が単位で直交し z = cross(x, y) * s だと、z の向きの拡大 s を落としていた)
        const bool rigid = std::fabs(dot(x, x) - 1) < 1e-12 && std::fabs(dot(y, y) - 1) < 1e-12 &&
                           std::fabs(dot(z, z) - 1) < 1e-12 && std::fabs(dot(x, y)) < 1e-12 &&
                           dot(cross(x, y), z) > 1 - 1e-12;
        const SmStatus st = rigid ? B(brep)->Transform(placement(t), nullptr, TRUE) : B(brep)->Transform(t, TRUE);
        if (st != SM_SUCCESS)
            throw Failure{"移動に失敗しました"};
    });
}

int jc_extrude_faces(const JcBrep* brep, const size_t* faces, size_t count, double from, double to, JcBrep** out)
{
    *out = nullptr;
    return guarded([&] {
        if (!(to > from))
            throw Failure{"押し出しの距離は 0 より大きくしてください"};
        const SmTArray<SmFace*> all = faces_of(B(brep));
        SmTArray<SmFace*> sel;
        for (size_t i = 0; i < count; ++i)
        {
            if (faces[i] >= all.GetSize())
                throw Failure{"押し出す面の番号が範囲の外です"};
            sel.Add(all[ULONG(faces[i])]);
        }
        if (sel.GetSize() == 0)
            throw Failure{"押し出す面がありません"};
        SmVector3d n;
        SmPoint3d p;
        if (!planar_normal(sel[0], n, &p)) // 外向きの法線
            throw Failure{"押し出す面が平らではありません"};
        // 面の縁をそのまま、外向きの法線の向きへ to − from だけ平行に動かす(縁の曲線は面の物を使う)
        SmTArray<SmPoint3d> pts;
        pts.Add(p);
        pts.Add(p + n * (to - from));
        SmTArray<double> knots;
        knots.Add(0.0);
        knots.Add(1.0);
        SmTArray<ULONG> mults;
        mults.Add(2);
        mults.Add(2);
        SmBSplineCurve* path = nullptr;
        check(SmApiCreateCanonicalCurve(pts, knots, mults, nullptr, 1, SM_CF_POLYLINE_FORM, path), "押し出す向きの線");
        std::unique_ptr<SmBSplineCurve> keep(path);
        SmSweepOptions opt;
        opt.bCapEndsArg = TRUE;
        opt.bTranslationalSweep = TRUE;
        opt.bMoveToProfile = TRUE;
        SmBrep* r = nullptr;
        check(SmApiCreateCurveSweepFromFaces(sel, path, nullptr, nullptr, &opt, r, nullptr, nullptr, nullptr),
              "面の押し出し");
        BrepPtr result(r);
        if (from != 0.0)
        {
            const SmVector3d d = n * from;
            double t[4][4] = {{1, 0, 0, 0}, {0, 1, 0, 0}, {0, 0, 1, 0}, {d.x, d.y, d.z, 1}};
            if (result->Transform(placement(t), nullptr, TRUE) != SM_SUCCESS)
                throw Failure{"面の押し出しの移動に失敗しました"};
        }
        *out = J(result.release());
    });
}

int jc_shell(const JcBrep* brep, const size_t* open, size_t open_count, double offset, JcBrep** out)
{
    return jc_shell_with(brep, open, open_count, offset, 3, out);
}

int jc_shell_with(const JcBrep* brep, const size_t* open, size_t open_count, double offset, int32_t flags,
                  JcBrep** out)
{
    *out = nullptr;
    return guarded([&] {
        const SmTArray<SmFace*> faces = faces_of(B(brep));
        SmTArray<const SmFace*> sel;
        for (size_t i = 0; i < open_count; ++i)
        {
            if (open[i] >= faces.GetSize())
                throw Failure{"開ける面の番号が範囲の外です"};
            sel.Add(faces[ULONG(open[i])]);
        }
        SmBrep* r = nullptr;
        const SmApiStatus st = SmApiShellBrepFull(B(brep), offset, (flags & 1) ? TRUE : FALSE,
                                                  (flags & 2) ? TRUE : FALSE, FALSE, sel, r);
        // SM_ERR(1001)は SMLib の一般の失敗で、理由は返らない(SmMessages.h)。実測では、厚みが角の丸めや
        // 円柱の半径以上の時(半径 1 の丸めで厚み 1 以上、半径 3 の円柱で 3 以上)と、厚みが形の厚さを
        // 超える時(高さ 10 の箱で 11)にこれになった(2026-10-21)。ほかの理由もありうるので断定しない。
        // 状態の番号は括弧に数字だけで書く(画面の知らせは、この形の括弧を外す。kmb-ui の plain_error)
        if (st == SM_ERR)
            throw Failure{"厚みが角の丸めや曲面の半径以上の所か、形の厚さを超える所があるかもしれません"
                          "(SMLib の状態 1001)"};
        check(st, "シェル");
        if (!r)
            throw Failure{"シェルの結果がありません"};
        *out = J(r);
    });
}

int jc_delete_fillet_faces(const JcBrep* brep, const size_t* faces, size_t count, JcBrep** out)
{
    *out = nullptr;
    return guarded([&] {
        SmBrep* c = nullptr;
        check(SmApiBrepCopy(B(brep), c), "形状の複製");
        BrepPtr copy(c);
        const SmTArray<SmFace*> all = faces_of(c);
        std::vector<SmFace*> sel;
        for (size_t i = 0; i < count; ++i)
        {
            if (faces[i] >= all.GetSize())
                throw Failure{"消す面の番号が範囲の外です"};
            // SMLib の丸めの見分け(SmFace::IdentifyFillet)は B-spline の面しか見ないので、JC の
            // フィレットが作る円筒の面を丸めと読まない。見分けは DeleteFilletFace に任せる(丸めの形
            // ――縁が 4 本で、向かい合う 2 本が接する辺――でなければ、そこで断られる)
            SmFace* f = all[ULONG(faces[i])];
            sel.push_back(f);
        }
        {
            SmTemporaryChangeValue<SmBoolean> edit(c->m_bEditingEnabled, TRUE);
            for (SmFace* f : sel)
                if (c->DeleteFilletFace(f) != SM_SUCCESS)
                    throw Failure{"丸めの面ではないか、まわりの面を延ばして交わらせられません"
                                  "(消せるのは平らな面と、フィレットでできた丸めの面です)"};
            check(c->StitchAndOrient(), "面のつなぎ直し");
        }
        *out = J(copy.release());
    });
}

int jc_face_count(const JcBrep* brep, size_t* out)
{
    return guarded([&] { *out = faces_of(B(brep)).GetSize(); });
}

int jc_faces(const JcBrep* brep, int32_t flags, JcFace* out, size_t cap)
{
    return guarded([&] {
        const SmTArray<SmFace*> faces = faces_of(B(brep));
        if (faces.GetSize() > cap)
            throw Failure{"面の数が変わりました"};
        for (ULONG i = 0; i < faces.GetSize(); ++i)
        {
            SmFace* f = faces[i];
            JcFace& o = out[i];
            std::memset(&o, 0, sizeof o);
            const SmLongAttribute* tag = static_cast<const SmLongAttribute*>(f->FindAttribute(kTagAttribute));
            o.tag = tag ? tag->GetValue() : -1;
            SmVector3d n;
            SmPoint3d on;
            o.planar = planar_normal(f, n, &on) ? 1 : 0;
            if (o.planar)
            {
                o.normal[0] = n.x;
                o.normal[1] = n.y;
                o.normal[2] = n.z;
                put(o.plane_point, on);
            }
            SmPoint3d p;
            if ((flags & JC_FACE_INNER_POINT) || ((flags & JC_FACE_INNER_POINT_CURVED) && !o.planar))
            {
                check(SmApiFaceInternalPoint(f, p), "面の内側の点");
            }
            else
            {
                SmPoint3d lo, hi;
                check(SmApiFaceBoundingBox(f, FALSE, lo, hi), "面の外形の箱");
                p = SmPoint3d((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, (lo.z + hi.z) / 2);
            }
            put(o.point, p);
            if (flags & JC_FACE_AREA)
                check(SmApiFaceComputeArea(f, 1e-4, o.area), "面の面積");
        }
    });
}

int jc_set_face_tags(JcBrep* brep, const int64_t* tags, size_t count)
{
    return guarded([&] {
        const SmTArray<SmFace*> faces = faces_of(B(brep));
        if (faces.GetSize() != count)
            throw Failure{"面の数が合いません"};
        const SmContext& ctx = *SmApiGetOrCreateContext();
        for (ULONG i = 0; i < faces.GetSize(); ++i)
        {
            SmFace* f = faces[i];
            if (SmAttribute* old = f->FindAttribute(kTagAttribute))
                f->RemoveAttribute(old);
            if (tags[i] >= 0)
                f->AddAttribute(new (ctx) SmLongAttribute(kTagAttribute, long(tags[i]), SM_AB_COPY));
        }
    });
}

void jc_free_buffer(void* p) { std::free(p); }

int jc_edges(const JcBrep* brep, JcEdge** out, size_t* count)
{
    *out = nullptr;
    *count = 0;
    return guarded([&] {
        const SmBrep* b = B(brep);
        const SmTArray<SmFace*> faces = faces_of(b);
        SmTArray<SmEdge*> edges;
        b->GetEdges(edges);
        std::vector<JcEdge> result;
        for (ULONG i = 0; i < edges.GetSize(); ++i)
        {
            SmEdge* e = edges[i];
            SmTArray<SmFace*> ef;
            e->GetFaces(ef);
            if (ef.GetSize() != 2 || ef[0] == ef[1])
                continue;
            SmVector3d na, nb;
            if (!planar_normal(ef[0], na) || !planar_normal(ef[1], nb))
                continue;
            // 平行でない 2 つの平面の境目は、まっすぐな線になる
            if (std::sqrt(dot(cross(na, nb), cross(na, nb))) < 1e-9)
                continue;
            SmVertex* v0 = nullptr;
            SmVertex* v1 = nullptr;
            e->GetVertices(v0, v1);
            if (!v0 || !v1 || v0 == v1)
                continue;
            SmPoint3d a, c;
            check(SmApiVertexGetPoint(v0, a), "頂点");
            check(SmApiVertexGetPoint(v1, c), "頂点");
            const SmVector3d t = unit(SmVector3d(c.x - a.x, c.y - a.y, c.z - a.z));
            // 面 A の中へ向かう向き: 辺と法線に直交する 2 つの向きのうち、面 A の上に乗る方
            SmVector3d in = unit(cross(na, t));
            const double len = std::sqrt((c.x - a.x) * (c.x - a.x) + (c.y - a.y) * (c.y - a.y) + (c.z - a.z) * (c.z - a.z));
            const double eps = std::min(0.01, len * 0.01);
            const SmPoint3d mid((a.x + c.x) / 2, (a.y + c.y) / 2, (a.z + c.z) / 2);
            auto on_face = [&](const SmVector3d& d) {
                SmPoint3d q(mid.x + d.x * eps, mid.y + d.y * eps, mid.z + d.z * eps), cp;
                double dist = 0;
                if (SmApiFaceClosestPoint(ef[0], q, cp, dist) != SM_SUCCESS)
                    return false;
                return dist < eps * 0.1;
            };
            if (!on_face(in))
            {
                in = SmVector3d(-in.x, -in.y, -in.z);
                if (!on_face(in))
                    continue;
            }
            JcEdge o;
            o.f0 = uint32_t(face_index(faces, ef[0]));
            o.f1 = ef.GetSize() == 1 ? UINT32_MAX : uint32_t(face_index(faces, ef[1]));
            put(o.a, a);
            put(o.b, c);
            o.convex = dot(in, nb) < 0 ? 1 : 0;
            result.push_back(o);
        }
        if (!result.empty())
        {
            *out = static_cast<JcEdge*>(std::malloc(sizeof(JcEdge) * result.size()));
            if (!*out)
                throw Failure{"メモリが足りません"};
            std::memcpy(*out, result.data(), sizeof(JcEdge) * result.size());
        }
        *count = result.size();
    });
}

int jc_face_distance(const JcBrep* brep, size_t face, const double p[3], double* out)
{
    return guarded([&] {
        const SmTArray<SmFace*> faces = faces_of(B(brep));
        if (face >= faces.GetSize())
            throw Failure{"面の番号が範囲の外です"};
        SmPoint3d cp;
        check(SmApiFaceClosestPoint(faces[ULONG(face)], SmPoint3d(p[0], p[1], p[2]), cp, *out), "面までの距離");
    });
}

int jc_arc_edges(const JcBrep* brep, JcArcEdge** out, size_t* count)
{
    *out = nullptr;
    *count = 0;
    return guarded([&] {
        const SmBrep* b = B(brep);
        const SmTArray<SmFace*> faces = faces_of(b);
        SmTArray<SmEdge*> edges;
        b->GetEdges(edges);
        std::vector<JcArcEdge> result;
        const auto sub3 = [](const SmPoint3d& p, const SmPoint3d& q) { return SmVector3d(p.x - q.x, p.y - q.y, p.z - q.z); };
        const auto len3 = [](const SmVector3d& v) { return std::sqrt(dot(v, v)); };
        const auto along = [](const SmPoint3d& p, const SmVector3d& d, double t) { return SmPoint3d(p.x + d.x * t, p.y + d.y * t, p.z + d.z * t); };
        for (ULONG i = 0; i < edges.GetSize(); ++i)
        {
            SmEdge* e = edges[i];
            SmTArray<SmFace*> ef;
            e->GetFaces(ef);
            if (ef.GetSize() != 2 || ef[0] == ef[1])
                continue;
            SmVector3d n0, n1;
            SmPoint3d pl;
            const bool p0 = planar_normal(ef[0], n0, &pl);
            const bool p1 = planar_normal(ef[1], n1, p0 ? nullptr : &pl);
            if (p0 == p1)
                continue;
            SmFace* fa = p0 ? ef[0] : ef[1];
            SmFace* fb = p0 ? ef[1] : ef[0];
            const SmVector3d n = p0 ? n0 : n1;
            const SmCurve* c = e->GetCurve();
            double t0 = 0, t1 = 0;
            if (!c || SmApiEdgeParameterRange(e, t0, t1) != SM_SUCCESS || !(t1 > t0))
                continue;
            // 辺の上の点を並べ、どれも平面の上で、同じ円の上にあるかを確かめる
            const int K = 16;
            std::vector<SmPoint3d> pts;
            bool ok = true;
            for (int k = 0; k <= K && ok; ++k)
            {
                SmVector3d pd[1];
                if (c->Evaluate(t0 + (t1 - t0) * k / K, 0, TRUE, pd) != SM_SUCCESS)
                    ok = false;
                else
                    pts.emplace_back(pd[0].x, pd[0].y, pd[0].z);
            }
            if (!ok)
                continue;
            const bool closed = len3(sub3(pts.front(), pts.back())) < 1e-7;
            const SmPoint3d& q0 = pts[0];
            const SmPoint3d& q1 = pts[closed ? K / 3 : K / 2];
            const SmPoint3d& q2 = pts[closed ? 2 * K / 3 : K];
            // 3 点を通る円の中心
            const SmVector3d u = sub3(q1, q0), v = sub3(q2, q0);
            const SmVector3d w = cross(u, v);
            const double ww = dot(w, w);
            if (ww < 1e-18)
                continue;
            const SmVector3d t = cross(w, u), s = cross(v, w);
            const double fu = dot(v, v) / (2 * ww), fv = dot(u, u) / (2 * ww);
            const SmPoint3d ctr(q0.x + t.x * fu + s.x * fv, q0.y + t.y * fu + s.y * fv, q0.z + t.z * fu + s.z * fv);
            const double r = len3(sub3(q0, ctr));
            const double tol = 1e-6 * std::max(1.0, r);
            if (!(r > 1e-6) || r > 1e6)
                continue;
            for (const SmPoint3d& p : pts)
                if (std::fabs(dot(sub3(p, pl), n)) > tol || std::fabs(len3(sub3(p, ctr)) - r) > tol)
                    ok = false;
            if (!ok)
                continue;
            // a から b への角度(軸 n まわり、反時計回りが正)
            double sweep = 0;
            SmVector3d prev = unit(sub3(pts[0], ctr));
            for (int k = 1; k <= K; ++k)
            {
                const SmVector3d cur = unit(sub3(pts[k], ctr));
                sweep += std::atan2(dot(cross(prev, cur), n), dot(prev, cur));
                prev = cur;
            }
            // 円筒の面が平面の外側(法線の向き)と内側のどちらへ伸びるか。円錐や球なら、どちらにも乗らない
            const double d = std::min(0.01, r * 0.01);
            const auto on = [&](SmFace* f, const SmPoint3d& q) {
                SmPoint3d cp;
                double dist = 0;
                return SmApiFaceClosestPoint(f, q, cp, dist) == SM_SUCCESS && dist < d * 0.05;
            };
            const SmPoint3d& mid = pts[K / 2];
            const SmPoint3d& mid2 = pts[K / 4];
            const bool down = on(fb, along(mid, n, -d)) && on(fb, along(mid2, n, -d));
            const bool up = on(fb, along(mid, n, d)) && on(fb, along(mid2, n, d));
            if (down == up)
                continue;
            const SmVector3d rad = unit(sub3(mid, ctr));
            const bool inside = on(fa, along(mid, rad, -d));
            if (inside == on(fa, along(mid, rad, d)))
                continue;
            JcArcEdge o;
            o.f0 = uint32_t(face_index(faces, fa));
            o.f1 = uint32_t(face_index(faces, fb));
            put(o.center, ctr);
            o.axis[0] = n.x;
            o.axis[1] = n.y;
            o.axis[2] = n.z;
            o.radius = r;
            const double tau = 6.283185307179586;
            o.sweep = closed ? (sweep < 0 ? -tau : tau) : sweep;
            put(o.a, pts.front());
            put(o.b, closed ? pts.front() : pts.back());
            o.inside = inside ? 1 : 0;
            o.convex = down ? 1 : 0;
            result.push_back(o);
        }
        if (!result.empty())
        {
            *out = static_cast<JcArcEdge*>(std::malloc(sizeof(JcArcEdge) * result.size()));
            if (!*out)
                throw Failure{"メモリが足りません"};
            std::memcpy(*out, result.data(), sizeof(JcArcEdge) * result.size());
        }
        *count = result.size();
    });
}

// 面 f の、点 p での外向きの法線
static bool face_normal_at(SmFace* f, const SmPoint3d& p, SmVector3d& n)
{
    const SmSurface* s = f->GetSurface();
    if (!s)
        return false;
    SmPoint3d cp;
    SmTArray<SmPoint2d> uvs;
    double dist = 0;
    if (SmApiSurfaceClosestPoint(s, p, cp, uvs, dist) != SM_SUCCESS || uvs.GetSize() == 0)
        return false;
    SmVector3d v;
    if (s->EvaluateNormal(SmVector2d(uvs[0].x, uvs[0].y), TRUE, TRUE, v) != SM_SUCCESS)
        return false;
    v = unit(v);
    const double sg = outward_sign(f);
    n = SmVector3d(v.x * sg, v.y * sg, v.z * sg);
    return true;
}

int jc_all_edges(const JcBrep* brep, JcAnyEdge** out, size_t* count, double** points, size_t* point_count)
{
    *out = nullptr;
    *count = 0;
    *points = nullptr;
    *point_count = 0;
    return guarded([&] {
        const SmBrep* b = B(brep);
        const SmTArray<SmFace*> faces = faces_of(b);
        SmTArray<SmEdge*> edges;
        b->GetEdges(edges);
        std::vector<JcAnyEdge> result;
        std::vector<double> pts;
        for (ULONG i = 0; i < edges.GetSize(); ++i)
        {
            SmEdge* e = edges[i];
            SmTArray<SmFace*> ef;
            e->GetFaces(ef);
            if ((ef.GetSize() != 2 && ef.GetSize() != 1) || (ef.GetSize() == 2 && (ef[0] == ef[1] || splits_one_surface(e))))
                continue;
            // 同じ面の両側を縫った周期の継ぎ目は、片側だけの開いた縁ではない。
            if (ef.GetSize() == 1 && e->IsManifold())
                continue;
            const SmCurve* c = e->GetCurve();
            double t0 = 0, t1 = 0;
            if (!c || SmApiEdgeParameterRange(e, t0, t1) != SM_SUCCESS || !(t1 > t0))
                continue;
            // まっすぐな辺は 2 点、曲がった辺は 32 区間
            SmVertex* v0 = nullptr;
            SmVertex* v1 = nullptr;
            e->GetVertices(v0, v1);
            const int K = c->IsLinear(1e-9) ? 1 : 32;
            JcAnyEdge o;
            o.f0 = uint32_t(face_index(faces, ef[0]));
            o.f1 = ef.GetSize() == 1 ? UINT32_MAX : uint32_t(face_index(faces, ef[1]));
            o.first = uint32_t(pts.size() / 3);
            o.count = uint32_t(K + 1);
            bool ok = true;
            for (int k = 0; k <= K && ok; ++k)
            {
                SmVector3d pd[1];
                if (c->Evaluate(t0 + (t1 - t0) * k / K, 0, TRUE, pd) != SM_SUCCESS)
                    ok = false;
                else
                {
                    pts.push_back(pd[0].x);
                    pts.push_back(pd[0].y);
                    pts.push_back(pd[0].z);
                }
            }
            if (!ok)
            {
                pts.resize(o.first * 3);
                continue;
            }
            // 出っ張りか: 真ん中で、面 A の上を辺から離れる向きが、面 B の外向きの法線と逆なら出っ張り
            const double tm = (t0 + t1) / 2;
            SmVector3d pd[2];
            SmVector3d na, nb;
            o.convex = 0;
            // 法線を求められなければ分からない(-1)。黙って角と答えない
            o.smooth = -1;
            if (ef.GetSize() == 2 && c->Evaluate(tm, 1, TRUE, pd) == SM_SUCCESS)
            {
                const SmPoint3d mid(pd[0].x, pd[0].y, pd[0].z);
                const SmVector3d t = unit(pd[1]);
                if (face_normal_at(ef[0], mid, na) && face_normal_at(ef[1], mid, nb))
                {
                    // 両側の外向きの法線がそろう(2° 以内。kmb_core::SEAM_DEG と同じ)なら、角の無い
                    // なめらかな継ぎ目。丸めの面は近似の曲面で、辺の真ん中の最寄りの点で見ると平面と
                    // 1° 余りずれることがある(1° では角と答え、シェルのつながる面を見落とした)
                    o.smooth = dot(unit(na), unit(nb)) >= 0.9993908270190958 ? 1 : 0;
                    SmVector3d in = unit(cross(na, t));
                    double len = 0;
                    SmApiEdgeComputeLength(e, 1e-6, len);
                    const double eps = std::min(0.01, std::max(len, 1e-3) * 0.01);
                    auto on_face = [&](const SmVector3d& d) {
                        SmPoint3d q(mid.x + d.x * eps, mid.y + d.y * eps, mid.z + d.z * eps), cp;
                        double dist = 0;
                        return SmApiFaceClosestPoint(ef[0], q, cp, dist) == SM_SUCCESS && dist < eps * 0.1;
                    };
                    if (!on_face(in))
                        in = SmVector3d(-in.x, -in.y, -in.z);
                    o.convex = dot(in, nb) < 0 ? 1 : 0;
                }
            }
            result.push_back(o);
        }
        if (!result.empty())
        {
            *out = static_cast<JcAnyEdge*>(std::malloc(sizeof(JcAnyEdge) * result.size()));
            *points = static_cast<double*>(std::malloc(sizeof(double) * pts.size()));
            if (!*out || !*points)
                throw Failure{"メモリが足りません"};
            std::memcpy(*out, result.data(), sizeof(JcAnyEdge) * result.size());
            std::memcpy(*points, pts.data(), sizeof(double) * pts.size());
        }
        *count = result.size();
        *point_count = pts.size() / 3;
    });
}

int jc_blend_edges(JcBrep* brep, const JcBlend* blends, size_t count, const double* radii)
{
    return guarded([&] {
        SmBrep* b = B(brep);
        const SmTArray<SmFace*> faces = faces_of(b);
        SmTArray<SmEdge*> all;
        b->GetEdges(all);
        SmContext* ctx = SmApiGetOrCreateContext();
        SmTArray<SmEdge*> edges;
        SmTArray<double> sizes;
        SmTArray<SmFilletSurfaceGeneratorType> kinds;
        SmTArray<SmFilletSolverType> solvers;
        SmTArray<SmFilletLaw*> laws;
        // 途中で変わる半径の決まり(丸め終わったら消す)
        std::vector<std::unique_ptr<SmFilletLaw>> owned;
        bool g2 = false;
        double weight = 1.0;
        for (size_t i = 0; i < count; ++i)
        {
            const JcBlend& bl = blends[i];
            if (bl.f0 >= faces.GetSize() || bl.f1 >= faces.GetSize())
                throw Failure{"丸める辺の面の番号が範囲の外です"};
            const SmFace* fa = faces[bl.f0];
            const SmFace* fb = faces[bl.f1];
            const SmPoint3d p(bl.point[0], bl.point[1], bl.point[2]);
            SmEdge* best = nullptr;
            double best_d = 1e300;
            for (ULONG j = 0; j < all.GetSize(); ++j)
            {
                SmTArray<SmFace*> ef;
                all[j]->GetFaces(ef);
                if (ef.GetSize() != 2 || !((ef[0] == fa && ef[1] == fb) || (ef[0] == fb && ef[1] == fa)))
                    continue;
                SmPoint3d cp;
                double t = 0, d = 0;
                if (SmApiEdgeClosestPoint(all[j], p, cp, t, d) == SM_SUCCESS && d < best_d)
                {
                    best = all[j];
                    best_d = d;
                }
            }
            if (!best)
                throw Failure{"丸める辺が見つかりません"};
            if (edges.IsIn(best))
                continue;
            SmFilletLaw* law = nullptr;
            SmFilletSolverType solver = SM_FS_CONST_RADIUS;
            if (bl.law == 1)
                solver = SM_FS_CONST_DIST;
            else if (bl.law == 2)
            {
                if (!radii || bl.count < 2)
                    throw Failure{"途中で変わる半径には、始めと終わりの半径が要ります"};
                // 位置 0 は start に近い端。辺の曲線の向き(パラメータの小さい端から)にそろえる
                double t0 = 0, t1 = 0;
                const SmCurve* c = best->GetCurve();
                if (!c || SmApiEdgeParameterRange(best, t0, t1) != SM_SUCCESS)
                    throw Failure{"辺の範囲を求められません"};
                SmVector3d a[1], z[1];
                if (c->Evaluate(t0, 0, TRUE, a) != SM_SUCCESS || c->Evaluate(t1, 0, TRUE, z) != SM_SUCCESS)
                    throw Failure{"辺の端を求められません"};
                auto d2 = [&](const SmVector3d& q) {
                    const double dx = q.x - bl.start[0], dy = q.y - bl.start[1], dz = q.z - bl.start[2];
                    return dx * dx + dy * dy + dz * dz;
                };
                const bool flip = d2(z[0]) < d2(a[0]);
                SmTArray<double> at, rs;
                for (uint32_t k = 0; k < bl.count; ++k)
                {
                    const uint32_t kk = flip ? bl.count - 1 - k : k;
                    const double pos = radii[2 * (bl.first + kk)];
                    at.Add(flip ? 1.0 - pos : pos);
                    rs.Add(radii[2 * (bl.first + kk) + 1]);
                }
                if (bl.count == 2)
                    law = new (*ctx) SmLinearFilletLaw(rs[0], rs[1]);
                else
                {
                    SmBoolean bad = FALSE;
                    // 2: 半径の点の間を、行き過ぎない(負にならない)3 次の曲線でつなぐ
                    law = new (*ctx) SmBSplineFilletLaw(*ctx, at, rs, 2, SmExtent1d(0.0, 1.0), bad);
                    if (bad)
                    {
                        delete law;
                        throw Failure{"途中で変わる半径の曲線を作れません"};
                    }
                }
                owned.emplace_back(law);
                solver = SM_FS_VARIABLE_RADIUS;
            }
            SmFilletSurfaceGeneratorType kind = SM_FSG_CIRCULAR;
            if (bl.flat)
                kind = SM_FSG_LINEAR;
            else if (bl.g2)
            {
                kind = SM_FSG_BLEND_CURVE;
                if (!g2)
                    weight = bl.weight > 0 ? bl.weight : 1.0;
                g2 = true;
            }
            edges.Add(best);
            sizes.Add(bl.size);
            kinds.Add(kind);
            solvers.Add(solver);
            laws.Add(law);
        }
        if (edges.GetSize() < 1)
            throw Failure{"丸める辺がありません"};
        // SmApiFilletEdgesPerEdge と同じ手順に、辺ごとの半径の決まりを足した
        SmFilletExecutive exec(*ctx, b);
        exec.SetDoGlobalMerge(TRUE);
        check(exec.SetFilletParameters(b, edges, kinds, solvers, sizes, &laws, -1.0, -1.0, -1.0, -1.0,
                                       g2 ? 2 : 1, weight),
              "フィレット");
        check(exec.DoFillet(), "フィレット");
        b->StitchAndOrient();
    });
}

int jc_mass(const JcBrep* brep, double relative_accuracy, JcMassProps* out)
{
    return guarded([&] {
        SmBrep* b = const_cast<SmBrep*>(B(brep));
        out->has_centroid = 0;
        check(SmApiBrepComputeVolume(b, relative_accuracy, out->volume), "体積の計算");
        // 重心は閉じた立体だけ(精度は [1e-4, 1e-1] に丸められる。体積は上の細かい方を使う)
        {
            double area = 0, volume = 0, mass = 0;
            SmPoint3d c;
            SmVector3d moments, products;
            if (SmApiBrepComputeMassProperties(b, std::max(relative_accuracy, 1e-4), 1.0, SmPoint3d(0, 0, 0), area,
                                               volume, mass, c, moments, products) == SM_SUCCESS)
            {
                put(out->centroid, c);
                out->has_centroid = 1;
            }
        }
        check(SmApiBrepComputeArea(b, std::max(relative_accuracy, 1e-4), out->area), "表面積の計算");
        SmPoint3d lo, hi;
        check(SmApiBrepBoundingBox(b, TRUE, lo, hi), "外形の箱");
        put(out->min, lo);
        put(out->max, hi);
    });
}

int jc_volume(const JcBrep* brep, double relative_accuracy, double* out)
{
    return guarded([&] {
        SmBrep* b = const_cast<SmBrep*>(B(brep));
        check(SmApiBrepComputeVolume(b, relative_accuracy, *out), "体積の計算");
    });
}

int jc_bounds(const JcBrep* brep, double min[3], double max[3])
{
    return guarded([&] {
        SmPoint3d lo, hi;
        check(SmApiBrepBoundingBox(B(brep), FALSE, lo, hi), "外形の箱");
        put(min, lo);
        put(max, hi);
    });
}

int jc_check(const JcBrep* brep, int32_t* manifold_solid, int64_t* solids, int64_t* voids)
{
    return guarded([&] {
        SmBoolean m = FALSE;
        check(SmApiBrepIsManifoldSolid(B(brep), m), "立体の検査");
        *manifold_solid = m ? 1 : 0;
        long s = 0, v = 0;
        check(SmApiBrepMaterialCensus(B(brep), s, v), "立体の数");
        *solids = s;
        *voids = v;
    });
}

void jc_mesh_free(JcMesh* mesh)
{
    if (!mesh)
        return;
    std::free(mesh->positions);
    std::free(mesh->normals);
    std::free(mesh->triangles);
    std::free(mesh->tri_face);
    std::free(mesh->edge_counts);
    std::free(mesh->edge_points);
    std::free(mesh);
}

int jc_mesh(const JcBrep* brep, double chord, double angle_deg, JcMesh** out)
{
    *out = nullptr;
    return guarded([&] {
        SmBrep* b = const_cast<SmBrep*>(B(brep));
        const SmTArray<SmFace*> faces = faces_of(b);
        SmTessellationParams params;
        params.dChordHeightTol = chord;
        params.dCurveAngleTolDeg = angle_deg;
        params.dSurfaceAngleTolDeg = angle_deg;
        SmPolyBrep* raw = nullptr;
        check(SmApiTessellate(b, raw, params), "三角形への分割");
        std::unique_ptr<SmPolyBrep> poly(raw);

        std::vector<float> pos, nor;
        std::vector<uint32_t> tris, tface;
        SmTArray<SmPolyFace*> pfaces;
        poly->GetPolyFaces(pfaces);
        for (ULONG i = 0; i < pfaces.GetSize(); ++i)
        {
            SmPolyFace* pf = pfaces[i];
            SmFace* orig = pf->GetOriginalFace();
            const long fi = face_index(faces, orig);
            const double sg = orig ? outward_sign(orig) : 1.0;
            SmPolyLoop* loop = pf->GetOuterPolyLoop();
            if (!loop)
                continue;
            SmTArray<SmPolyEdge*> pedges;
            loop->GetPolyEdges(pedges);
            const ULONG n = pedges.GetSize();
            if (n < 3)
                continue;
            const SmVector3d flat = pf->GetNormal();
            const uint32_t base = uint32_t(pos.size() / 3);
            for (ULONG j = 0; j < n; ++j)
            {
                SmPolyVertex* v = pedges[j]->GetStartPolyVertex();
                const SmPoint3d p = v->GetPoint();
                pos.insert(pos.end(), {float(p.x), float(p.y), float(p.z)});
                // 頂点の法線: 元の面の曲面の法線があればそれ、無ければ多角形の法線
                SmVector3d nn = flat;
                SmTArray<SmTopology*>& vf = v->GetFacesRef();
                SmTArray<SmVector3d>& vn = v->GetNormalsRef();
                for (ULONG k = 0; k < vf.GetSize() && k < vn.GetSize(); ++k)
                {
                    if (vf[k] == static_cast<SmTopology*>(orig) || vf[k] == static_cast<SmTopology*>(pf))
                    {
                        const SmVector3d s = vn[k];
                        nn = vf[k] == static_cast<SmTopology*>(orig) ? SmVector3d(s.x * sg, s.y * sg, s.z * sg) : s;
                        break;
                    }
                }
                if (dot(nn, flat) < 0)
                    nn = SmVector3d(-nn.x, -nn.y, -nn.z);
                nor.insert(nor.end(), {float(nn.x), float(nn.y), float(nn.z)});
            }
            const uint32_t id = fi < 0 ? 0xffffffffu : uint32_t(fi);
            if (n == 3)
            {
                tris.insert(tris.end(), {base, base + 1, base + 2});
                tface.push_back(id);
                continue;
            }
            // 平面へ写す(法線にいちばん近い軸を落とす。落とした軸の符号で回る向きを揃える)
            const double ax = std::fabs(flat.x), ay = std::fabs(flat.y), az = std::fabs(flat.z);
            const int drop = az >= ax && az >= ay ? 2 : (ax >= ay ? 0 : 1);
            const double sg2 = (drop == 0 ? flat.x : drop == 1 ? flat.y : flat.z) < 0 ? -1.0 : 1.0;
            std::vector<std::array<double, 2>> flat2(n);
            for (ULONG j = 0; j < n; ++j)
            {
                const float* q = &pos[(base + j) * 3];
                const double x = q[0], y = q[1], z = q[2];
                flat2[j] = drop == 2 ? std::array<double, 2>{x, y * sg2}
                         : drop == 0 ? std::array<double, 2>{y, z * sg2}
                                     : std::array<double, 2>{z, x * sg2};
            }
            for (uint32_t k : ear_clip(flat2))
                tris.push_back(base + k);
            tface.resize(tris.size() / 3, id);
        }

        SmTArray<SmPoint3d> bpts;
        SmTArray<ULONG> bcounts;
        SmTArray<SmEdge*> bedges;
        std::vector<uint32_t> counts;
        std::vector<float> epts;
        if (SmApiTessellateBoundaries(b, angle_deg, bpts, bcounts, bedges) == SM_SUCCESS)
        {
            ULONG k = 0;
            for (ULONG i = 0; i < bcounts.GetSize(); ++i)
            {
                // 円柱などの閉じた曲面の継ぎ目(1 つの面が両側にある辺)は、曲面の作り方の都合で
                // できる線で、形の上に特別な所は無い。描かない(選ぶ・吸い付く対象にもしない)
                const bool seam = i < bedges.GetSize() && bedges[i] && (bedges[i]->IsSeam() || splits_one_surface(bedges[i]));
                if (!seam)
                    counts.push_back(uint32_t(bcounts[i]));
                for (ULONG j = 0; j < bcounts[i]; ++j, ++k)
                    if (!seam)
                        epts.insert(epts.end(), {float(bpts[k].x), float(bpts[k].y), float(bpts[k].z)});
            }
        }

        JcMesh* m = static_cast<JcMesh*>(std::calloc(1, sizeof(JcMesh)));
        if (!m)
            throw Failure{"メモリが足りません"};
        auto copy = [](const auto& v, auto*& dst) {
            using T = typename std::decay_t<decltype(v)>::value_type;
            dst = static_cast<T*>(std::malloc(sizeof(T) * (v.size() ? v.size() : 1)));
            if (dst && !v.empty())
                std::memcpy(dst, v.data(), sizeof(T) * v.size());
        };
        m->vertex_count = pos.size() / 3;
        copy(pos, m->positions);
        copy(nor, m->normals);
        m->triangle_count = tface.size();
        copy(tris, m->triangles);
        copy(tface, m->tri_face);
        m->edge_count = counts.size();
        copy(counts, m->edge_counts);
        copy(epts, m->edge_points);
        if (!m->positions || !m->normals || !m->triangles || !m->tri_face || !m->edge_counts || !m->edge_points)
        {
            jc_mesh_free(m);
            throw Failure{"メモリが足りません"};
        }
        *out = m;
    });
}

} // extern "C"
