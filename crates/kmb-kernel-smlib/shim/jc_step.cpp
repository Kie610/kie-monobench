// STEP(ISO 10303-21、AP214)の書き出し。説明は jc_smlib.h の jc_export_step。
//
// smlibpy(MIT、commit 3695509)の src/smlibpy/step.py と src/cpp/ext.cpp の brep_dump を
// C++ に移したもの。平面・円筒・円錐・球・円環と、直線・円・楕円はそのままの形で、
// それ以外は B スプラインで書く(曲面に解析形でないものが 1 つでもあれば、複製を
// まるごと B スプラインに直してから書く)。立体は MANIFOLD_SOLID_BREP(空洞があれば
// BREP_WITH_VOIDS)にする。
//
// 元にした smlibpy の許諾: Copyright (c) 2026 AWV Informatik AG、MIT License
// (全文は third_party/smlibpy-patches/LICENSE)。

#include "jc_smlib.h"

#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiQueries.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmCylinder.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmEllipse.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmLine.h>
#include <SmLoopuse.h>
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSphere.h>
#include <SmTorus.h>
#include <SmVertex.h>
#include <SmVertexuse.h>

#include <algorithm>
#include <charconv>
#include <cstdint>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <exception>
#include <stdexcept>
#include <map>
#include <memory>
#include <string>
#include <tuple>
#include <unordered_map>
#include <vector>

// jc_smlib.cpp の中にある共通の後始末(例外を文字列にして返す)を使う
int jc_guarded_call(void (*body)(void*), void* data);

namespace
{
struct StepFailure
{
    std::string message;
};

void check(SmStatus st, const char* what)
{
    if (st != SM_SUCCESS)
        throw StepFailure{std::string("STEP の書き出し: ") + what + " に失敗しました(SMLib の状態 " +
                          std::to_string(st) + ")"};
}

const SmContext& ctx() { return *SmApiGetOrCreateContext(); }

using V3 = SmVector3d;

double dot(const V3& a, const V3& b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
V3 cross(const V3& a, const V3& b)
{
    return V3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
}
V3 sub(const V3& a, const V3& b) { return V3(a.x - b.x, a.y - b.y, a.z - b.z); }
V3 add(const V3& a, const V3& b) { return V3(a.x + b.x, a.y + b.y, a.z + b.z); }
V3 mul(const V3& a, double s) { return V3(a.x * s, a.y * s, a.z * s); }
double norm(const V3& a) { return std::sqrt(dot(a, a)); }
V3 unit(const V3& a)
{
    const double l = norm(a);
    return l > 0 ? mul(a, 1.0 / l) : a;
}

// STEP の実数の書き方(最短で元に戻る桁、指数は E、必ず小数点を付ける)
std::string real(double v)
{
    if (!std::isfinite(v))
        throw StepFailure{"STEP の書き出し: 有限でない数があります"};
    if (v == 0.0)
        return "0.";
    char buf[64];
    auto res = std::to_chars(buf, buf + sizeof buf, v);
    std::string s(buf, res.ptr);
    const auto e = s.find_first_of("eE");
    if (e != std::string::npos)
    {
        std::string mant = s.substr(0, e);
        const int ex = std::atoi(s.c_str() + e + 1);
        if (mant.find('.') == std::string::npos)
            mant += ".";
        return mant + "E" + std::to_string(ex);
    }
    if (s.find('.') == std::string::npos)
        s += ".";
    return s;
}

// STEP の文字列。' と \ は 2 つにし、ASCII 以外(UTF-8)は続く字ごとに \X2\…\X0\(UTF-16 の 16 進)、
// 基本面の外の字は \X4\…\X0\(UTF-32)で書く。UTF-8 として読めないバイトと制御文字は ? にする
std::string text(const std::string& t)
{
    static const char hex[] = "0123456789ABCDEF";
    std::string out = "'";
    int open = 0; // 開いている \X2\(2)か \X4\(4)
    auto close = [&] {
        if (open)
            out += "\\X0\\";
        open = 0;
    };
    auto wide = [&](uint32_t v, int kind) {
        if (open != kind)
        {
            close();
            out += kind == 2 ? "\\X2\\" : "\\X4\\";
            open = kind;
        }
        for (int i = kind * 2 - 1; i >= 0; --i)
            out += hex[(v >> (4 * i)) & 0xF];
    };
    const size_t n = t.size();
    for (size_t i = 0; i < n;)
    {
        const unsigned char c = t[i];
        if (c < 0x80)
        {
            close();
            if (c == '\'')
                out += "''";
            else if (c == '\\')
                out += "\\\\";
            else if (c < 0x20 || c == 0x7f)
                out += '?';
            else
                out += char(c);
            ++i;
            continue;
        }
        const int len = c >= 0xF0 ? 4 : c >= 0xE0 ? 3 : c >= 0xC0 ? 2 : 0;
        uint32_t v = len == 4 ? c & 0x07 : len == 3 ? c & 0x0F : c & 0x1F;
        bool ok = len > 0 && i + len <= n;
        for (int k = 1; ok && k < len; ++k)
        {
            const unsigned char d = t[i + k];
            ok = (d & 0xC0) == 0x80;
            v = (v << 6) | (d & 0x3F);
        }
        if (!ok || v > 0x10FFFF || (v >= 0xD800 && v <= 0xDFFF))
        {
            close();
            out += '?';
            ++i;
            continue;
        }
        wide(v, v > 0xFFFF ? 4 : 2);
        i += len;
    }
    close();
    return out + "'";
}

std::string ref(int id) { return "#" + std::to_string(id); }

template <class T, class F>
std::string list(const std::vector<T>& items, F f)
{
    std::string s = "(";
    for (size_t i = 0; i < items.size(); ++i)
    {
        if (i)
            s += ",";
        s += f(items[i]);
    }
    return s + ")";
}

std::string refs(const std::vector<int>& ids)
{
    return list(ids, [](int i) { return ref(i); });
}

struct Writer
{
    std::vector<std::string> lines;
    std::map<std::tuple<double, double, double>, int> points, dirs;

    int add(std::string line)
    {
        lines.push_back(std::move(line));
        return int(lines.size());
    }

    int point(const V3& p)
    {
        auto key = std::make_tuple(p.x, p.y, p.z);
        auto it = points.find(key);
        if (it != points.end())
            return it->second;
        const int id = add("CARTESIAN_POINT('',(" + real(p.x) + "," + real(p.y) + "," + real(p.z) + "))");
        points[key] = id;
        return id;
    }

    int direction(const V3& d)
    {
        const V3 v = unit(d);
        auto key = std::make_tuple(v.x, v.y, v.z);
        auto it = dirs.find(key);
        if (it != dirs.end())
            return it->second;
        const int id = add("DIRECTION('',(" + real(v.x) + "," + real(v.y) + "," + real(v.z) + "))");
        dirs[key] = id;
        return id;
    }

    int axis2(const V3& origin, const V3& z, const V3& x)
    {
        return add("AXIS2_PLACEMENT_3D(''," + ref(point(origin)) + "," + ref(direction(z)) + "," +
                   ref(direction(x)) + ")");
    }
};

// z と、z に直交させた x
void orthonormal(V3& z, V3& x)
{
    z = unit(z);
    x = unit(sub(x, mul(z, dot(x, z))));
}

void placement(const SmAxis2Placement& a, V3& origin, V3& z, V3& x)
{
    origin = a.GetOriginRef();
    x = a.GetXAxisRef();
    z = cross(a.GetXAxisRef(), a.GetYAxisRef());
    orthonormal(z, x);
}

// 重なった節点をまとめる
void knot_mults(const SmTArray<double>& uniq_in, const SmTArray<ULONG>& mult_in, std::vector<double>& uniq,
                std::vector<int>& mult)
{
    // SMLib の GetCanonical は節点を重なりなしで返すが、念のため近いものをまとめる
    for (ULONG i = 0; i < uniq_in.GetSize(); ++i)
    {
        const double k = uniq_in[i];
        if (!uniq.empty() && std::fabs(k - uniq.back()) <= 1e-14 * std::max(1.0, std::fabs(k)))
            mult.back() += int(mult_in[i]);
        else
        {
            uniq.push_back(k);
            mult.push_back(int(mult_in[i]));
        }
    }
}

std::string ints(const std::vector<int>& v)
{
    return list(v, [](int i) { return std::to_string(i); });
}

std::string reals(const std::vector<double>& v)
{
    return list(v, [](double d) { return real(d); });
}

bool analytic_surface(const SmSurface* s)
{
    switch (s->GetType())
    {
    case SmPlane_TYPE:
    case SmCylinder_TYPE:
    case SmCone_TYPE:
    case SmSphere_TYPE:
    case SmTorus_TYPE:
        return true;
    default:
        return s->IsKindOf(SmBSplineSurface_TYPE) != FALSE;
    }
}

// 面の外向きの法線を STEP の解析形の曲面の法線と比べるための情報
struct Sense
{
    enum Kind { Plane, Cylinder, Cone, Sphere, Torus, Native } kind = Native;
    V3 origin, z;
    double radius = 0, angle = 0;
};

class BodyWriter
{
public:
    BodyWriter(Writer& w, const SmBrep* b) : w_(w), brep_(b)
    {
        b->GetVertices(vertices_);
        b->GetEdges(edges_);
        b->GetFaces(faces_);
        b->GetRegions(regions_);
    }

    // 立体ごとに MANIFOLD_SOLID_BREP か BREP_WITH_VOIDS を書き、その番号を返す
    std::vector<int> solids(const std::string& name)
    {
        // 殻の番号は領域の順に付ける
        std::unordered_map<const SmShell*, int> sidx;
        std::vector<std::vector<int>> region_shells(regions_.GetSize());
        for (ULONG r = 0; r < regions_.GetSize(); ++r)
        {
            SmTArray<SmShell*> shells;
            regions_[r]->GetShells(shells);
            for (ULONG s = 0; s < shells.GetSize(); ++s)
            {
                const int id = int(sidx.size());
                sidx[shells[s]] = id;
                region_shells[r].push_back(id);
            }
        }
        // 殻ごとの面: 殻の中の面使用はその領域を向くので、外を向く相方の面使用で書く
        std::map<int, std::vector<std::pair<int, const SmFaceuse*>>> shell_faces;
        for (ULONG f = 0; f < faces_.GetSize(); ++f)
        {
            SmFaceuse* up = faces_[f]->GetUpwardFaceuse();
            SmFaceuse* down = up ? up->GetMate() : nullptr;
            for (SmFaceuse* use : {up, down})
            {
                if (!use || !use->GetShell())
                    continue;
                auto it = sidx.find(use->GetShell());
                if (it == sidx.end())
                    continue;
                const SmFaceuse* mate = use == up ? down : up;
                if (mate)
                    shell_faces[it->second].push_back({int(f), mate});
            }
        }
        std::vector<int> items;
        for (ULONG r = 0; r < regions_.GetSize(); ++r)
        {
            const SmRegion* region = regions_[r];
            if (region == brep_->GetInfiniteRegion() || region->IsVoid() == TRUE)
                continue;
            std::vector<int> shells;
            for (int si : region_shells[r])
            {
                std::vector<int> fs;
                for (auto& [fi, mate] : shell_faces[si])
                    fs.push_back(face(fi, mate));
                if (!fs.empty())
                    shells.push_back(w_.add("CLOSED_SHELL(''," + refs(fs) + ")"));
            }
            if (shells.empty())
                continue;
            if (shells.size() == 1)
                items.push_back(w_.add("MANIFOLD_SOLID_BREP(" + text(name) + "," + ref(shells[0]) + ")"));
            else
            {
                std::vector<int> voids;
                for (size_t i = 1; i < shells.size(); ++i)
                    voids.push_back(w_.add("ORIENTED_CLOSED_SHELL('',*," + ref(shells[i]) + ",.F.)"));
                items.push_back(w_.add("BREP_WITH_VOIDS(" + text(name) + "," + ref(shells[0]) + "," + refs(voids) +
                                       ")"));
            }
        }
        return items;
    }

private:
    int vertex(const SmVertex* v)
    {
        auto it = vertex_ids_.find(v);
        if (it != vertex_ids_.end())
            return it->second;
        const int id = w_.add("VERTEX_POINT(''," + ref(w_.point(v->GetPoint())) + ")");
        vertex_ids_[v] = id;
        return id;
    }

    int bspline_curve(const SmBSplineCurve* c)
    {
        ULONG dim = 0, degree = 0;
        SmTArray<SmPoint3d> poles;
        SmBSplineCurveForm form;
        SmTArray<ULONG> mults_in;
        SmTArray<double> knots_in, weights;
        SmKnotType knot_type;
        check(c->GetCanonical(dim, degree, poles, form, mults_in, knots_in, knot_type, weights),
              "B スプライン曲線の読み出し");
        std::vector<int> pts;
        for (ULONG i = 0; i < poles.GetSize(); ++i)
            pts.push_back(w_.point(poles[i]));
        std::vector<double> uniq;
        std::vector<int> mult;
        knot_mults(knots_in, mults_in, uniq, mult);
        const std::string deg = std::to_string(degree);
        if (weights.GetSize() == 0)
            return w_.add("B_SPLINE_CURVE_WITH_KNOTS(''," + deg + "," + refs(pts) +
                          ",.UNSPECIFIED.,.F.,.F.," + ints(mult) + "," + reals(uniq) + ",.UNSPECIFIED.)");
        std::vector<double> ws;
        for (ULONG i = 0; i < weights.GetSize(); ++i)
            ws.push_back(weights[i]);
        return w_.add("(BOUNDED_CURVE() B_SPLINE_CURVE(" + deg + "," + refs(pts) +
                      ",.UNSPECIFIED.,.F.,.F.) B_SPLINE_CURVE_WITH_KNOTS(" + ints(mult) + "," + reals(uniq) +
                      ",.UNSPECIFIED.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(" +
                      reals(ws) + ") REPRESENTATION_ITEM(''))");
    }

    // 辺の曲線と、辺の向き(始点 → 終点)が曲線の向きと同じか
    std::pair<int, bool> curve(const SmEdge* e)
    {
        const SmCurve* c = e->GetCurve();
        if (!c)
            throw StepFailure{"STEP の書き出し: 曲線の無い辺があります"};
        const SmExtent1d ivl = e->GetInterval();
        if (c->GetType() == SmLine_TYPE)
        {
            V3 pd[2];
            check(c->Evaluate(ivl.GetMin(), 0, TRUE, pd), "直線の評価");
            const V3 a = pd[0];
            check(c->Evaluate(ivl.GetMax(), 0, FALSE, pd), "直線の評価");
            const V3 d = sub(pd[0], a);
            const double len = norm(d);
            if (len <= 0)
                throw StepFailure{"STEP の書き出し: 長さ 0 の直線の辺があります"};
            const int vec = w_.add("VECTOR(''," + ref(w_.direction(d)) + "," + real(len) + ")");
            return {w_.add("LINE(''," + ref(w_.point(a)) + "," + ref(vec) + ")"), true};
        }
        if (c->GetType() == SmCircle_TYPE || c->GetType() == SmEllipse_TYPE)
        {
            SmAxis2Placement a;
            V3 o, z, x;
            int ent = 0;
            if (c->GetType() == SmCircle_TYPE)
            {
                double r = 0;
                check(static_cast<const SmCircle*>(c)->GetCanonical(a, r), "円の読み出し");
                placement(a, o, z, x);
                ent = w_.add("CIRCLE(''," + ref(w_.axis2(o, z, x)) + "," + real(r) + ")");
            }
            else
            {
                double r1 = 0, r2 = 0;
                check(static_cast<const SmEllipse*>(c)->GetCanonical(a, r1, r2), "楕円の読み出し");
                placement(a, o, z, x);
                if (r1 >= r2)
                    ent = w_.add("ELLIPSE(''," + ref(w_.axis2(o, z, x)) + "," + real(r1) + "," + real(r2) + ")");
                else // STEP は 1 つ目の半径を長い方にするので、軸を 90° 回す
                    ent = w_.add("ELLIPSE(''," + ref(w_.axis2(o, z, cross(z, x))) + "," + real(r2) + "," +
                                 real(r1) + ")");
            }
            // STEP の円と楕円は軸のまわりに反時計回り
            V3 pd[2];
            check(c->Evaluate(0.5 * (ivl.GetMin() + ivl.GetMax()), 1, TRUE, pd), "円の評価");
            const V3 ccw = cross(z, sub(pd[0], o));
            return {ent, dot(pd[1], ccw) > 0};
        }
        if (c->IsKindOf(SmBSplineCurve_TYPE))
            return {bspline_curve(static_cast<const SmBSplineCurve*>(c)), true};
        // 合成・オフセット・面の上の曲線など: 媒介変数を保った B スプラインで近似する
        SmTArray<double> breaks;
        const SmExtent1d nat = c->GetNaturalInterval();
        breaks.Add(nat.GetMin());
        breaks.Add(nat.GetMax());
        double gap = 0;
        SmBSplineCurve* approx = nullptr;
        check(c->ApproximateCurve(ctx(), breaks, 1e-7, gap, approx, FALSE, TRUE),
              "曲線の近似");
        std::unique_ptr<SmBSplineCurve> keep(approx);
        return {bspline_curve(approx), true};
    }

    int edge(const SmEdge* e)
    {
        auto it = edge_ids_.find(e);
        if (it != edge_ids_.end())
            return it->second;
        SmVertex* v0 = e->GetStartVertex();
        SmVertex* v1 = e->GetEndVertex();
        if (!v0 || !v1)
            throw StepFailure{"STEP の書き出し: 頂点の無い辺があります"};
        auto [crv, same] = curve(e);
        const int id = w_.add("EDGE_CURVE(''," + ref(vertex(v0)) + "," + ref(vertex(v1)) + "," + ref(crv) + "," +
                              (same ? ".T." : ".F.") + ")");
        edge_ids_[e] = id;
        return id;
    }

    int bspline_surface(const SmBSplineSurface* s)
    {
        ULONG udeg = 0, vdeg = 0;
        SmTArray<SmPoint3d> poles;
        SmBSplineSurfaceForm form;
        SmTArray<ULONG> umults, vmults;
        SmTArray<double> uknots, vknots, weights;
        SmKnotType knot_type;
        check(s->GetCanonical(udeg, vdeg, poles, form, umults, vmults, uknots, vknots, knot_type, weights),
              "B スプライン曲面の読み出し");
        std::vector<double> uu, vu;
        std::vector<int> um, vm;
        knot_mults(uknots, umults, uu, um);
        knot_mults(vknots, vmults, vu, vm);
        int nu = -int(udeg) - 1, nv = -int(vdeg) - 1;
        for (int m : um)
            nu += m;
        for (int m : vm)
            nv += m;
        if (nu <= 0 || nv <= 0 || ULONG(nu * nv) != poles.GetSize())
            throw StepFailure{"STEP の書き出し: B スプライン曲面の制御点の数が合いません"};
        std::string net = "(";
        for (int i = 0; i < nu; ++i)
        {
            std::vector<int> row;
            for (int j = 0; j < nv; ++j)
                row.push_back(w_.point(poles[ULONG(i * nv + j)]));
            net += (i ? "," : "") + refs(row);
        }
        net += ")";
        const std::string head = std::to_string(udeg) + "," + std::to_string(vdeg) + "," + net;
        const std::string knots = ints(um) + "," + ints(vm) + "," + reals(uu) + "," + reals(vu);
        if (weights.GetSize() == 0)
            return w_.add("B_SPLINE_SURFACE_WITH_KNOTS(''," + head + ",.UNSPECIFIED.,.F.,.F.,.F.," + knots +
                          ",.UNSPECIFIED.)");
        std::string wrows = "(";
        for (int i = 0; i < nu; ++i)
        {
            std::vector<double> row;
            for (int j = 0; j < nv; ++j)
                row.push_back(weights[ULONG(i * nv + j)]);
            wrows += (i ? "," : "") + reals(row);
        }
        wrows += ")";
        return w_.add("(BOUNDED_SURFACE() B_SPLINE_SURFACE(" + head + ",.UNSPECIFIED.,.F.,.F.,.F.) "
                      "B_SPLINE_SURFACE_WITH_KNOTS(" + knots + ",.UNSPECIFIED.) "
                      "GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(" + wrows +
                      ") REPRESENTATION_ITEM('') SURFACE())");
    }

    int surface(const SmSurface* s, Sense& sense)
    {
        SmAxis2Placement a;
        V3 o, z, x;
        switch (s->GetType())
        {
        case SmPlane_TYPE:
            check(static_cast<const SmPlane*>(s)->GetCanonical(a), "平面の読み出し");
            placement(a, o, z, x);
            sense.kind = Sense::Plane;
            sense.z = z;
            return w_.add("PLANE(''," + ref(w_.axis2(o, z, x)) + ")");
        case SmCylinder_TYPE:
        {
            double r = 0;
            check(static_cast<const SmCylinder*>(s)->GetCanonical(a, r), "円筒の読み出し");
            placement(a, o, z, x);
            sense.kind = Sense::Cylinder;
            sense.origin = o;
            sense.z = z;
            return w_.add("CYLINDRICAL_SURFACE(''," + ref(w_.axis2(o, z, x)) + "," + real(r) + ")");
        }
        case SmCone_TYPE:
        {
            double r = 0, semi = 0;
            check(static_cast<const SmCone*>(s)->GetCanonical(a, r, semi), "円錐の読み出し");
            placement(a, o, z, x);
            sense.origin = o;
            if (std::fabs(semi) < 1e-12)
            {
                sense.kind = Sense::Cylinder;
                sense.z = z;
                return w_.add("CYLINDRICAL_SURFACE(''," + ref(w_.axis2(o, z, x)) + "," + real(r) + ")");
            }
            if (semi < 0)
            {
                z = mul(z, -1.0);
                semi = -semi;
            }
            const double ang = semi * 3.14159265358979323846 / 180.0;
            sense.kind = Sense::Cone;
            sense.z = z;
            sense.radius = r;
            sense.angle = ang;
            return w_.add("CONICAL_SURFACE(''," + ref(w_.axis2(o, z, x)) + "," + real(r) + "," + real(ang) + ")");
        }
        case SmSphere_TYPE:
        {
            double r = 0;
            check(static_cast<const SmSphere*>(s)->GetCanonical(a, r), "球の読み出し");
            placement(a, o, z, x);
            sense.kind = Sense::Sphere;
            sense.origin = o;
            return w_.add("SPHERICAL_SURFACE(''," + ref(w_.axis2(o, z, x)) + "," + real(r) + ")");
        }
        case SmTorus_TYPE:
        {
            double big = 0, small = 0;
            check(static_cast<const SmTorus*>(s)->GetCanonical(a, big, small), "円環の読み出し");
            placement(a, o, z, x);
            sense.kind = Sense::Torus;
            sense.origin = o;
            sense.z = z;
            sense.radius = big;
            return w_.add("TOROIDAL_SURFACE(''," + ref(w_.axis2(o, z, x)) + "," + real(big) + "," + real(small) +
                          ")");
        }
        default:
            break;
        }
        if (s->IsKindOf(SmBSplineSurface_TYPE))
        {
            sense.kind = Sense::Native;
            return bspline_surface(static_cast<const SmBSplineSurface*>(s));
        }
        throw StepFailure{"STEP の書き出し: 書けない種類の曲面があります"};
    }

    // 面使用の法線(カーネルの曲面の法線を面使用の向きで裏返したもの)が、
    // STEP の解析形の曲面の法線と同じ向きか
    bool same_sense(const SmFace* f, const SmFaceuse* use, const Sense& s)
    {
        const bool use_same = use->GetOrientation() == SM_OT_SAME;
        if (s.kind == Sense::Native)
            return use_same;
        const SmSurface* srf = f->GetSurface();
        SmPoint3d p;
        check(SmApiFaceInternalPoint(f, p), "面の内側の点");
        SmPoint3d cp;
        SmTArray<SmPoint2d> uvs;
        double dist = 0;
        check(SmApiSurfaceClosestPoint(srf, p, cp, uvs, dist), "曲面の最寄りの点");
        if (uvs.GetSize() == 0)
            throw StepFailure{"STEP の書き出し: 曲面の上の点が見つかりません"};
        SmPoint2d uv = uvs[0];
        if (s.kind == Sense::Cone || s.kind == Sense::Torus)
        {
            const V3 d = sub(p, s.origin);
            if (norm(sub(d, mul(s.z, dot(d, s.z)))) < 1e-9 * std::max(1.0, norm(d)))
            {
                // 軸の上の点(円錐の頂点)では半径の向きが決まらないので、面の真ん中を使う
                uv = f->GetUVDomain().GetMid();
                SmVector3d der[1];
                check(srf->Evaluate(uv, 0, 0, TRUE, TRUE, TRUE, der), "曲面の評価");
                p = der[0];
            }
        }
        SmVector3d n;
        check(srf->EvaluateNormal(uv, TRUE, TRUE, n), "曲面の法線");
        const V3 n_face = use_same ? n : mul(n, -1.0);
        V3 n_step;
        switch (s.kind)
        {
        case Sense::Plane:
            n_step = s.z;
            break;
        case Sense::Cylinder:
        {
            const V3 d = sub(p, s.origin);
            n_step = sub(d, mul(s.z, dot(d, s.z)));
            break;
        }
        case Sense::Cone:
        {
            const V3 d = sub(p, s.origin);
            const V3 radial = unit(sub(d, mul(s.z, dot(d, s.z))));
            n_step = sub(mul(radial, std::cos(s.angle)), mul(s.z, std::sin(s.angle)));
            break;
        }
        case Sense::Sphere:
            n_step = sub(p, s.origin);
            break;
        case Sense::Torus:
        {
            const V3 d = sub(p, s.origin);
            const V3 radial = unit(sub(d, mul(s.z, dot(d, s.z))));
            n_step = sub(p, add(s.origin, mul(radial, s.radius)));
            break;
        }
        case Sense::Native:
            break;
        }
        return dot(n_face, n_step) > 0;
    }

    // 辺使用 eu をたどり始める所で、円筒・円錐の軸のまわりに角度の増える向き(反時計回り)へ進むなら 1、
    // 逆なら -1、決まらなければ 0
    static int turn(const SmEdgeuse* eu, const Sense& s)
    {
        const SmEdge* e = eu->GetEdge();
        const SmCurve* c = e ? e->GetCurve() : nullptr;
        if (!c)
            return 0;
        const bool fwd = eu->GetOrientation() == SM_OT_SAME;
        const SmExtent1d ivl = e->GetInterval();
        V3 pd[2];
        if (c->Evaluate(fwd ? ivl.GetMin() : ivl.GetMax(), 1, fwd ? TRUE : FALSE, pd) != SM_SUCCESS)
            return 0;
        const V3 d = sub(pd[0], s.origin);
        const V3 radial = sub(d, mul(s.z, dot(d, s.z)));
        const double t = dot(cross(radial, fwd ? pd[1] : mul(pd[1], -1.0)), s.z);
        const double scale = norm(radial) * norm(pd[1]);
        if (!(scale > 0) || std::fabs(t) < 1e-9 * scale)
            return 0;
        return t > 0 ? 1 : -1;
    }

    // 1 周する円筒・円錐の面の輪(輪・継ぎ目・輪・継ぎ目)は、継ぎ目を初めて使った後の輪が角度の
    // 増える向きに回るように並べる。読み手は継ぎ目の初めの使いを角度 0 の側に置いて次の輪を広げる
    // ので、逆に回ると面が欠ける。逆なら次の輪から始める
    static void seam_first(std::vector<const SmEdgeuse*>& eus, const Sense& s)
    {
        if (s.kind != Sense::Cylinder && s.kind != Sense::Cone)
            return;
        const size_t n = eus.size();
        for (size_t i = 0; i < n; ++i)
        {
            const SmEdge* e = eus[i]->GetEdge();
            size_t uses = 0;
            for (const SmEdgeuse* x : eus)
                uses += x->GetEdge() == e ? 1 : 0;
            if (uses != 2)
                continue;
            const size_t j = (i + 1) % n;
            if (turn(eus[j], s) < 0)
                std::rotate(eus.begin(), eus.begin() + std::ptrdiff_t(j), eus.end());
            return;
        }
    }

    // 面使用 use から見た ADVANCED_FACE(法線は面使用の法線)
    int face(int fi, const SmFaceuse* use)
    {
        const SmFace* f = faces_[ULONG(fi)];
        Sense sense;
        const int srf = surface(f->GetSurface(), sense);
        std::vector<int> bounds;
        SmTArray<SmLoopuse*> lus;
        use->GetLoopuses(lus);
        for (ULONG l = 0; l < lus.GetSize(); ++l)
        {
            const SmLoopuse* lu = lus[l];
            int lp = 0;
            if (lu->IsVertexLoopuse())
            {
                SmVertexuse* vu = lu->GetVertexuse();
                if (!vu || !vu->GetVertex())
                    throw StepFailure{"STEP の書き出し: 頂点の無い輪があります"};
                lp = w_.add("VERTEX_LOOP(''," + ref(vertex(vu->GetVertex())) + ")");
            }
            else
            {
                SmTArray<SmEdgeuse*> got;
                lu->GetEdgeuses(got);
                std::vector<const SmEdgeuse*> eus;
                for (ULONG k = 0; k < got.GetSize(); ++k)
                    eus.push_back(got[k]);
                seam_first(eus, sense);
                std::vector<int> oes;
                for (const SmEdgeuse* eu : eus)
                {
                    const bool fwd = eu->GetOrientation() == SM_OT_SAME;
                    oes.push_back(w_.add("ORIENTED_EDGE('',*,*," + ref(edge(eu->GetEdge())) + "," +
                                         (fwd ? ".T." : ".F.") + ")"));
                }
                lp = w_.add("EDGE_LOOP(''," + refs(oes) + ")");
            }
            bounds.push_back(
                w_.add(std::string(l == 0 ? "FACE_OUTER_BOUND" : "FACE_BOUND") + "(''," + ref(lp) + ",.T.)"));
        }
        const bool same = same_sense(f, use, sense);
        return w_.add("ADVANCED_FACE(''," + refs(bounds) + "," + ref(srf) + "," + (same ? ".T." : ".F.") + ")");
    }

    Writer& w_;
    const SmBrep* brep_;
    SmTArray<SmVertex*> vertices_;
    SmTArray<SmEdge*> edges_;
    SmTArray<SmFace*> faces_;
    SmTArray<SmRegion*> regions_;
    std::unordered_map<const SmVertex*, int> vertex_ids_;
    std::unordered_map<const SmEdge*, int> edge_ids_;
};

struct BrepDeleter
{
    void operator()(SmBrep* b) const { delete b; }
};

std::vector<int> write_body(Writer& w, const SmBrep* b, const std::string& name)
{
    bool needs_nurbs = false;
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    for (ULONG i = 0; i < faces.GetSize(); ++i)
        if (!faces[i]->GetSurface() || !analytic_surface(faces[i]->GetSurface()))
            needs_nurbs = true;
    if (!needs_nurbs)
    {
        BodyWriter bw(w, b);
        return bw.solids(name);
    }
    SmBrep* raw = nullptr;
    check(SmApiBrepCopy(b, raw), "形状の複製");
    std::unique_ptr<SmBrep, BrepDeleter> copy(raw);
    check(copy->TurnToNURBS(), "B スプラインへの変換");
    BodyWriter bw(w, copy.get());
    return bw.solids(name);
}

std::string export_step(const JcBrep* const* breps, const char* const* names, size_t count, const char* product)
{
    Writer w;
    const std::string prod = product && *product ? product : "part";
    const int app = w.add("APPLICATION_CONTEXT('core data for automotive mechanical design processes')");
    w.add("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000," + ref(app) + ")");
    const int pctx = w.add("PRODUCT_CONTEXT(''," + ref(app) + ",'mechanical')");
    const int prd = w.add("PRODUCT(" + text(prod) + "," + text(prod) + ",'',(" + ref(pctx) + "))");
    w.add("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(" + ref(prd) + "))");
    const int form = w.add("PRODUCT_DEFINITION_FORMATION('',''," + ref(prd) + ")");
    const int dctx = w.add("PRODUCT_DEFINITION_CONTEXT('part definition'," + ref(app) + ",'design')");
    const int pdef = w.add("PRODUCT_DEFINITION('design',''," + ref(form) + "," + ref(dctx) + ")");
    const int pshape = w.add("PRODUCT_DEFINITION_SHAPE('',''," + ref(pdef) + ")");
    const int length = w.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))");
    const int angle = w.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))");
    const int solid_angle = w.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())");
    const int unc = w.add("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07)," + ref(length) +
                          ",'distance_accuracy_value','confusion accuracy')");
    const int rctx = w.add("(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((" + ref(unc) +
                           ")) GLOBAL_UNIT_ASSIGNED_CONTEXT((" + ref(length) + "," + ref(angle) + "," +
                           ref(solid_angle) +
                           ")) REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))");
    const int origin = w.axis2(V3(0, 0, 0), V3(0, 0, 1), V3(1, 0, 0));
    std::vector<int> items;
    for (size_t i = 0; i < count; ++i)
    {
        const std::string label =
            names && names[i] && *names[i] ? names[i] : (count == 1 ? prod : prod + "_" + std::to_string(i + 1));
        const auto body = write_body(w, reinterpret_cast<const SmBrep*>(breps[i]), label);
        items.insert(items.end(), body.begin(), body.end());
    }
    if (items.empty())
        throw StepFailure{"STEP の書き出し: 書く立体がありません"};
    std::vector<int> rep_items{origin};
    rep_items.insert(rep_items.end(), items.begin(), items.end());
    const int rep =
        w.add("ADVANCED_BREP_SHAPE_REPRESENTATION(" + text(prod) + "," + refs(rep_items) + "," + ref(rctx) + ")");
    w.add("SHAPE_DEFINITION_REPRESENTATION(" + ref(pshape) + "," + ref(rep) + ")");

    char stamp[32] = "";
    const std::time_t now = std::time(nullptr);
    if (const std::tm* t = std::gmtime(&now))
        std::strftime(stamp, sizeof stamp, "%Y-%m-%dT%H:%M:%S", t);
    std::string out = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('kieMonoBench model'),'2;1');\n";
    out += "FILE_NAME(" + text(prod) + "," + text(stamp) + ",(''),(''),'kieMonoBench','kieMonoBench (SMLib)','');\n";
    out += "FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n";
    for (size_t i = 0; i < w.lines.size(); ++i)
        out += "#" + std::to_string(i + 1) + "=" + w.lines[i] + ";\n";
    out += "ENDSEC;\nEND-ISO-10303-21;\n";
    return out;
}

struct ExportCall
{
    const JcBrep* const* breps;
    const char* const* names;
    size_t count;
    const char* product;
    char** out;
};

void run_export(void* data)
{
    auto* c = static_cast<ExportCall*>(data);
    std::string s;
    try
    {
        s = export_step(c->breps, c->names, c->count, c->product);
    }
    catch (const StepFailure& e)
    {
        throw std::runtime_error(e.message);
    }
    char* buf = static_cast<char*>(std::malloc(s.size() + 1));
    if (!buf)
        throw std::runtime_error("メモリが足りません");
    std::memcpy(buf, s.c_str(), s.size() + 1);
    *c->out = buf;
}
} // namespace

extern "C" int jc_export_step(const JcBrep* const* breps, const char* const* names, size_t count,
                              const char* product, char** out)
{
    *out = nullptr;
    ExportCall call{breps, names, count, product, out};
    return jc_guarded_call(run_export, &call);
}
