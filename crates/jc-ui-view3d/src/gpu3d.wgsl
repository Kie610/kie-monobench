
struct U {
    right: vec4<f32>, up: vec4<f32>, eye: vec4<f32>, center: vec4<f32>, light: vec4<f32>,
    ao: vec4<f32>, back_top: vec4<f32>, back_low: vec4<f32>,
    proj: vec4<f32>, pan: vec4<f32>, misc: vec4<f32>,
    section: vec4<f32>, extra: vec4<f32>,
    lit_dir: array<vec4<f32>, 3>, lit_rad: array<vec4<f32>, 3>,
    top: vec4<f32>, horizon: vec4<f32>, below: vec4<f32>, fx: vec4<f32>,
    looks: array<Look, 128>,
    lamps: array<Lamp, 8>,
};
struct Look { a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d0: vec4<f32>, d1: vec4<f32>, d2: vec4<f32> };
struct Lamp { p: vec4<f32>, c: vec4<f32>, d: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
// 入り隅の陰の前の段で描いた、画素ごとの見えている面の位置(4 つ目が 1 なら面がある)
@group(1) @binding(0) var gpos: texture_2d<f32>;
// 設計の画面の環境マップ(jc_render::envmap と同じ絵。粗さごとの段は mip)と拡散の光の絵
@group(1) @binding(1) var env_tex: texture_2d<f32>;
@group(1) @binding(2) var irr_tex: texture_2d<f32>;
@group(1) @binding(3) var env_smp: sampler;
// デカールの絵(層ごとに 1 枚。jc_render::decal と同じ貼り方)
@group(1) @binding(4) var decal_tex: texture_2d_array<f32>;

// ---- 写実の見た目(jc_render::env・jc_core::material と同じ式) ----
const PI: f32 = 3.14159265;

fn backdrop(d: vec3<f32>) -> vec3<f32> {
    if (d.z >= 0.0) {
        return mix(u.horizon.xyz, u.top.xyz, pow(d.z, 0.6));
    }
    return mix(u.horizon.xyz, u.below.xyz, smoothstep(0.0, 0.08, -d.z));
}

fn diffuse_backdrop(n: vec3<f32>) -> vec3<f32> {
    let sky = mix(u.horizon.xyz, u.top.xyz, 0.45);
    return mix(u.below.xyz, sky, 0.5 + 0.5 * n.z) * u.fx.y;
}

fn light_diffuse(k: i32, n: vec3<f32>) -> vec3<f32> {
    let l = u.lit_dir[k];
    let half = l.w;
    let c = dot(n, l.xyz);
    let w = smoothstep(-half, half, c) * max(c, half * 0.5);
    let omega = 2.0 * PI * (1.0 - u.lit_rad[k].w);
    return u.lit_rad[k].xyz * (u.fx.y * omega * w / PI);
}

// 点光源とスポットライトの拡散の光(影は見ない。jc_render::lamp::Lamp::white_at と同じ式)
fn lamp_diffuse(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var c = vec3<f32>(0.0);
    let count = i32(u.fx.w);
    for (var k = 0; k < count; k = k + 1) {
        let lp = u.lamps[k];
        let to = lp.p.xyz - p;
        let d2 = max(max(dot(to, to), lp.p.w * lp.p.w), 1e-6);
        let l = to / sqrt(d2);
        let cs = dot(n, l);
        if (cs <= 0.0) {
            continue;
        }
        var spot = 1.0;
        if (lp.c.w >= -1.0) {
            spot = smoothstep(lp.c.w, lp.d.w, dot(-l, lp.d.xyz));
        }
        c = c + lp.c.xyz * (cs * spot / (PI * d2));
    }
    return c;
}

// デカールを重ねた色(線形)と覆う割合(w)。面の平面へまっすぐ投影し、裏を向いた所には貼らない。
// 印刷は金属でないので、呼ぶ側は覆う割合だけ金属らしさを下げる
fn decal_over(lk: Look, albedo: vec3<f32>, p: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    if (lk.d0.w < 0.5) {
        return vec4<f32>(albedo, 0.0);
    }
    let nd = normalize(cross(lk.d1.xyz, lk.d2.xyz));
    if (dot(n, nd) <= 0.05) {
        return vec4<f32>(albedo, 0.0);
    }
    let q = p - lk.d0.xyz;
    var s = dot(q, lk.d1.xyz);
    var t = dot(q, lk.d2.xyz);
    if (s < 0.0 || s > 1.0 || t < 0.0 || t > 1.0) {
        return vec4<f32>(albedo, 0.0);
    }
    let flags = i32(round(lk.d2.w));
    if ((flags & 1) != 0) {
        s = 1.0 - s;
    }
    if ((flags & 2) != 0) {
        t = 1.0 - t;
    }
    let c = textureSampleLevel(decal_tex, env_smp, vec2<f32>(s, 1.0 - t), i32(round(lk.d0.w)) - 1, 0.0);
    let cover = c.a * lk.d1.w;
    return vec4<f32>(mix(albedo, c.rgb, cover), cover);
}

fn env_specular(r: vec3<f32>, rough: f32) -> vec3<f32> {
    let lobe = min(rough * rough * 1.6 + 0.01, 1.4);
    var c = mix(backdrop(r), diffuse_backdrop(r) / max(u.fx.y, 1e-6), smoothstep(0.0, 1.2, lobe));
    let count = i32(u.below.w);
    for (var k = 0; k < count; k = k + 1) {
        let l = u.lit_dir[k];
        let a = acos(clamp(dot(r, l.xyz), -1.0, 1.0));
        let cover = 1.0 - smoothstep(abs(l.w - lobe), l.w + lobe, a);
        let ratio = pow(min(l.w / lobe, 1.0), 2.0);
        c = c + u.lit_rad[k].xyz * (cover * ratio);
    }
    return c * u.fx.y;
}

fn env_brdf(rough: f32, ndv: f32) -> vec2<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = rough * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * max(ndv, 0.0))) * r.x + r.y;
    return vec2<f32>(-1.04, 1.04) * a004 + r.zw;
}

fn hash3(i: vec3<i32>) -> f32 {
    var h = bitcast<u32>(i.x) * 374761393u + bitcast<u32>(i.y) * 668265263u + bitcast<u32>(i.z) * 1274126177u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffu) / 32767.5 - 1.0;
}

fn noise3(p: vec3<f32>) -> f32 {
    let f = floor(p);
    let i = vec3<i32>(f);
    let x = p - f;
    let t = x * x * (3.0 - 2.0 * x);
    let x00 = mix(hash3(i), hash3(i + vec3<i32>(1, 0, 0)), t.x);
    let x10 = mix(hash3(i + vec3<i32>(0, 1, 0)), hash3(i + vec3<i32>(1, 1, 0)), t.x);
    let x01 = mix(hash3(i + vec3<i32>(0, 0, 1)), hash3(i + vec3<i32>(1, 0, 1)), t.x);
    let x11 = mix(hash3(i + vec3<i32>(0, 1, 1)), hash3(i + vec3<i32>(1, 1, 1)), t.x);
    return mix(mix(x00, x10, t.y), mix(x01, x11, t.y), t.z);
}

// 晩材の濃さ(jc_core::material::grain_amount と同じ式)
fn grain_of(p: vec3<f32>, lk: Look) -> f32 {
    let ring = abs(lk.c.w);
    let along_y = lk.c.w < 0.0;
    let d0 = p - lk.c.xyz;
    // 芯の向きを x、横切る向きを y にそろえる
    var d = d0;
    if (along_y) {
        d = vec3<f32>(d0.y, d0.x, d0.z);
    }
    let r = sqrt(d.y * d.y + d.z * d.z);
    let q = vec3<f32>(d.x * 0.012, d.y * 0.03, d.z * 0.03);
    var qq = q;
    if (along_y) {
        qq = vec3<f32>(q.y, q.x, q.z);
    }
    let wobble = noise3(qq) * 2.2 + noise3(qq * 3.1) * 0.5;
    let rr = r / max(ring, 0.1) + wobble;
    let t = rr - floor(rr);
    let late = smoothstep(0.55, 0.85, t) * (1.0 - smoothstep(0.93, 1.0, t));
    let s0 = vec3<f32>(d.x * 0.08, d.y * 1.6, d.z * 1.6);
    var s = s0;
    if (along_y) {
        s = vec3<f32>(s0.y, s0.x, s0.z);
    }
    let streak = pow(noise3(s) * 0.5 + 0.5, 3.0) * 0.35;
    return clamp(late * 0.85 + streak, 0.0, 1.0);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}

fn tonemap(x0: vec3<f32>) -> vec3<f32> {
    let x = max(x0, vec3<f32>(0.0));
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// 材質の面の色(sRGB)。jc_render::raster::shade_surface の影なしと同じ
fn shade_look(lk: Look, p: vec3<f32>, n0: vec3<f32>, view: vec3<f32>) -> vec3<f32> {
    var n = normalize(n0);
    let n_out = n;
    if (dot(n, view) < 0.0) {
        n = -n;
    }
    var albedo = lk.a.xyz;
    if (lk.c.w != 0.0) {
        // 木目の色は sRGB の間で混ぜてから線形にする(CPU と同じ)
        let base = to_srgb(lk.a.xyz);
        let dark = to_srgb(lk.b.xyz);
        albedo = to_linear(mix(base, dark, grain_of(p, lk)));
    }
    let dc = decal_over(lk, albedo, p, n_out);
    albedo = dc.xyz;
    let metal = clamp(lk.a.w, 0.0, 1.0) * (1.0 - dc.w);
    let rough = clamp(lk.b.w, 0.03, 1.0);
    let ndv = max(dot(n, view), 1e-4);
    let r = 2.0 * dot(n, view) * n - view;
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    var diff = diffuse_backdrop(n) + lamp_diffuse(p, n);
    let count = i32(u.below.w);
    for (var k = 0; k < count; k = k + 1) {
        diff = diff + light_diffuse(k, n);
    }
    let ab = env_brdf(rough, ndv);
    let spec = env_specular(r, rough) * (f0 * ab.x + vec3<f32>(ab.y));
    return to_srgb(tonemap(albedo * (1.0 - metal) * diff + spec));
}

// ---- 設計の画面の環境マップ(jc_render::envmap::EnvMap::shade と同じ式) ----
const ENV_LEVELS: f32 = 6.0;
const ENV_EXPOSURE: f32 = 0.7;
const STEEL_ROUGHNESS: f32 = 0.12;

// 世界の向きの絵の上の位置。景色は世界に固定(右 = +X、手前 = −Y、上 = +Z。EnvFrame::world)
fn env_uv(d: vec3<f32>) -> vec2<f32> {
    let l = vec3<f32>(d.x, -d.y, d.z);
    return vec2<f32>(atan2(l.y, l.x) / (2.0 * PI) + 0.5, acos(clamp(l.z, -1.0, 1.0)) / PI);
}

// 面の色(線形、トーンマップの前)
fn shade_env(albedo: vec3<f32>, metal: f32, rough: f32, n: vec3<f32>, view: vec3<f32>) -> vec3<f32> {
    let ndv = max(dot(n, view), 1e-4);
    let r = 2.0 * dot(n, view) * n - view;
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let diff = albedo * (1.0 - metal) * textureSampleLevel(irr_tex, env_smp, env_uv(n), 0.0).xyz;
    let ab = env_brdf(rough, ndv);
    let fade = pow(clamp(1.0 + 1.1 * dot(r, n), 0.0, 1.0), 2.0);
    let lod = clamp(rough, 0.0, 1.0) * (ENV_LEVELS - 1.0);
    let spec = textureSampleLevel(env_tex, env_smp, env_uv(r), lod).xyz * (f0 * ab.x + vec3<f32>(ab.y));
    return (diff + spec * fade) * ENV_EXPOSURE;
}

// 環境マップで引く時の視線: 平行投影でも、面の中の濃淡の仮の視点(view3d::FlatLight と同じ)へ向ける
fn env_view(p: vec3<f32>) -> vec3<f32> {
    if (u.proj.y > 0.0) {
        return toward_eye(p);
    }
    let e = u.eye.xyz;
    let d = p - u.center.xyz;
    let lat = d - e * dot(d, e);
    return normalize(e * u.light.w - lat);
}

// 材質の面の色(sRGB)を環境マップで(設計の画面)
fn shade_look_env(lk: Look, p: vec3<f32>, n0: vec3<f32>) -> vec3<f32> {
    let view = env_view(p);
    var n = normalize(n0);
    if (dot(n, view) < 0.0) {
        n = -n;
    }
    var albedo = lk.a.xyz;
    if (lk.c.w != 0.0) {
        albedo = to_linear(mix(to_srgb(lk.a.xyz), to_srgb(lk.b.xyz), grain_of(p, lk)));
    }
    let dc = decal_over(lk, albedo, p, normalize(n0));
    let metal = clamp(lk.a.w, 0.0, 1.0) * (1.0 - dc.w);
    return to_srgb(tonemap(shade_env(dc.xyz, metal, clamp(lk.b.w, 0.03, 1.0), n, view)));
}

// 面の点から視点へ向かう向き
fn toward_eye(p: vec3<f32>) -> vec3<f32> {
    if (u.proj.y <= 0.0) {
        return u.eye.xyz;
    }
    return normalize(u.center.xyz + u.eye.xyz * u.proj.y - p);
}

// 断面で切り取った側の点か(view3d::Section::removes と同じ)
fn cut(p: vec3<f32>) -> bool {
    return u.extra.x > 0.5 && dot(p, u.section.xyz) > u.section.w;
}

// 画面の上の位置(point。ビューの真ん中から)。点線の刻みに使う
fn screen(c: vec4<f32>) -> vec2<f32> {
    return c.xy / c.w * u.proj.zw;
}

// CPU の Camera::project と同じ写し方。bias は視点の側へ寄せる量(mm)
fn clip(p: vec3<f32>, bias: f32) -> vec4<f32> {
    let d = p - u.center.xyz;
    let x = dot(d, u.right.xyz);
    let y = dot(d, u.up.xyz);
    let z = dot(d, u.eye.xyz) + bias;
    let zoom = u.proj.x;
    let dist = u.proj.y;
    let hw = u.proj.z;
    let hh = u.proj.w;
    if (dist <= 0.0) {
        let depth = clamp((u.pan.z - z) / (2.0 * u.pan.z), 0.0, 1.0);
        return vec4<f32>((x * zoom + u.pan.x) / hw, (y * zoom - u.pan.y) / hh, depth, 1.0);
    }
    let t = dist - z;
    let k = zoom * dist;
    let near = u.pan.w;
    let far = u.misc.x;
    return vec4<f32>(
        (x * k + u.pan.x * t) / hw,
        (y * k - u.pan.y * t) / hh,
        far / (far - near) * (t - near),
        t,
    );
}

struct FaceOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs_face(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> FaceOut {
    var o: FaceOut;
    o.pos = clip(p, 0.0);
    o.normal = n;
    o.color = c;
    o.world = p;
    return o;
}

// 切り口(裏の面): 少し手前へ寄せる(view3d::CAP_BIAS。接するボディの面と競わない)
@vertex
fn vs_cap(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> FaceOut {
    var o: FaceOut;
    o.pos = clip(p, u.misc.y * 0.3);
    o.normal = n;
    o.color = c;
    o.world = p;
    return o;
}

@fragment
fn fs_face(i: FaceOut) -> @location(0) vec4<f32> {
    if (i.color.a <= 0.0 || cut(i.world)) {
        discard;
    }
    // 色の 4 つ目が 1〜127 なら材質の番号(写実の見た目)
    let slot = i32(round(i.color.a * 255.0));
    if (u.fx.x > 0.5 && slot >= 1 && slot <= 127) {
        if (u.fx.z > 0.5) {
            return vec4<f32>(shade_look_env(u.looks[slot - 1], i.world, i.normal), 1.0);
        }
        let c = shade_look(u.looks[slot - 1], i.world, i.normal, toward_eye(i.world));
        return vec4<f32>(c, 1.0);
    }
    // 253 は既定の鋼(外観の無いボディ): 頂点の色を地の色に、金属として環境マップで描く。
    // 入り隅の陰は今の描き方と同じくらいの強さで掛ける
    if (slot == 253 && u.fx.z > 0.5) {
        let view = env_view(i.world);
        var n = normalize(i.normal);
        if (dot(n, view) < 0.0) {
            n = -n;
        }
        let c = to_srgb(tonemap(shade_env(to_linear(i.color.rgb), 1.0, STEEL_ROUGHNESS, n, view)));
        let vis = ao_vis(i.pos.xy, i.normal, i.world);
        return vec4<f32>(c * (1.0 - 0.6 * (1.0 - vis)), 1.0);
    }
    // CPU の描画と同じ明るさ(view3d::FlatLight。CPU には入り隅の陰が無い)。
    // 色の掛け算は sRGB のまま(CPU の描画と同じ)
    let s = flat_shade(i.normal, i.world, ao_vis(i.pos.xy, i.normal, i.world));
    var k = s.x;
    var shine = s.y;
    // 254 は淡く描くボディ: 陰を弱める(view3d::FADE_FLAT と同じ)
    if (slot == 254) {
        k = mix(k, 1.0, 0.7);
        shine = shine * 0.3;
    }
    return vec4<f32>(clamp(i.color.rgb * k + vec3<f32>(shine), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// 質感で描かない面の明るさの倍率と白いつや(view3d::FlatLight::shade と同じ式と値)
fn flat_shade(n0: vec3<f32>, p: vec3<f32>, vis: f32) -> vec2<f32> {
    let e = u.eye.xyz;
    var n = normalize(n0);
    if (dot(n, e) < 0.0) {
        n = -n;
    }
    let key = u.light.xyz;
    let diff = max(dot(n, key), 0.0);
    let front = max(dot(n, e), 0.0);
    let shine = pow(max(dot(n, normalize(key + e)), 0.0), 40.0) * 0.25;
    // 面の中の濃淡: 画面の上の位置から仮の視点へ向かう向きの反射と、平行投影の向きの反射の差
    let d = p - u.center.xyz;
    let lat = d - e * dot(d, e);
    let vv = normalize(e * u.light.w - lat);
    let env = (smoothstep(0.0, 1.0, reflect(-vv, n).z) - smoothstep(0.0, 1.0, reflect(-e, n).z)) * 0.4;
    let k = 0.42 + 0.38 * diff + 0.25 * front + env - (0.42 + 0.25 * front) * (1.0 - vis);
    return vec2<f32>(k, shine);
}

// 入り隅の陰: 画面の上で半径 u.ao.x(mm)の円の中の面の位置を拾い、法線の側にある面ほど遮るとする。
// 見える割合(1 は遮られていない)を返す
fn ao_vis(px: vec2<f32>, n0: vec3<f32>, p: vec3<f32>) -> f32 {
    let r = u.ao.x;
    if (u.ao.y <= 0.0 || r <= 0.0) {
        return 1.0;
    }
    let size = vec2<i32>(textureDimensions(gpos));
    let q0 = clamp(vec2<i32>(px), vec2<i32>(0), size - vec2<i32>(1));
    // 前の段で描いた面でなければ(道具の立体・切り口・縁の画素)陰を付けない
    let own = textureLoad(gpos, q0, 0);
    if (own.w < 0.5 || length(own.xyz - p) > r * 0.25) {
        return 1.0;
    }
    var n = normalize(n0);
    if (dot(n, toward_eye(p)) < 0.0) {
        n = -n;
    }
    // 半径を画面の画素の数へ直す(透視投影でも同じ写し方)
    let a = clip(p, 0.0);
    let b = clip(p + u.right.xyz * r, 0.0);
    let rad = length((b.xy / b.w - a.xy / a.w) * u.proj.zw) * u.misc.z;
    let count = max(i32(u.ao.z), 1);
    var occ = 0.0;
    for (var i = 0; i < count; i = i + 1) {
        let fi = f32(i);
        // 黄金角で回しながら外へ広げ、画素ごとに始めの角をずらす(縞を出さない)
        let ang = fi * 2.399963 + f32((q0.x * 7 + q0.y * 13) % 17) * 0.37;
        let rr = sqrt((fi + 0.5) / f32(count)) * rad;
        let q = q0 + vec2<i32>(i32(cos(ang) * rr), i32(sin(ang) * rr));
        if (any(q < vec2<i32>(0)) || any(q >= size)) {
            continue;
        }
        let s = textureLoad(gpos, q, 0);
        if (s.w < 0.5) {
            continue;
        }
        let d = s.xyz - p;
        let dist = length(d);
        if (dist < r * 1e-4) {
            continue;
        }
        let c = max(dot(n, d / dist) - u.ao.w, 0.0);
        occ = occ + c * (1.0 - smoothstep(r * 0.5, r * 1.5, dist));
    }
    return clamp(1.0 - u.ao.y * occ / f32(count), 0.0, 1.0);
}

// 背景: 画面いっぱいの三角形 1 つで、上の端から下の端へ色を変える(view3d::backdrop と同じ)
struct BackOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_back(@builtin(vertex_index) k: u32) -> BackOut {
    let x = f32(i32(k & 1u) * 4 - 1);
    let y = f32(i32(k >> 1u) * 4 - 1);
    var o: BackOut;
    o.pos = vec4<f32>(x, y, 1.0, 1.0);
    o.t = (1.0 - y) * 0.5;
    return o;
}

@fragment
fn fs_back(i: BackOut) -> @location(0) vec4<f32> {
    return vec4<f32>(mix(u.back_top.rgb, u.back_low.rgb, clamp(i.t, 0.0, 1.0)), 1.0);
}

// 切り口の縞にかかる割合(view3d::hatch と同じ式。p は画素の真ん中の位置)
fn hatch(p: vec2<f32>) -> f32 {
    let ppp = max(u.misc.z, 0.001);
    let gap = 6.0;
    let width = 1.4;
    let t = (p.x + p.y) / sqrt(2.0) / ppp;
    let d = abs(fract(t / gap) - 0.5) * gap;
    return clamp((width / 2.0 - d) * ppp + 0.5, 0.0, 1.0);
}

// 断面で切った時に見える裏の面: 切り口の色で平らに塗り、斜めの縞を入れる(縞は view3d::hatch_color)
@fragment
fn fs_cap(i: FaceOut) -> @location(0) vec4<f32> {
    if (cut(i.world)) {
        discard;
    }
    let stripe = floor(i.color.rgb * 255.0 * 0.6) / 255.0;
    return vec4<f32>(mix(i.color.rgb, stripe, hatch(i.pos.xy)), 1.0);
}

// 透かす道具の立体: 面と同じ明るさで、色の透明度のまま混ぜる
@fragment
fn fs_ghost(i: FaceOut) -> @location(0) vec4<f32> {
    let l = dot(normalize(i.normal), u.light.xyz);
    // 透ける立体は陰を弱く(view3d::ghost_shade と同じ式)
    let k = select(0.88, 0.9 + 0.1 * l, l >= 0.0);
    return vec4<f32>(clamp(i.color.rgb * k, vec3<f32>(0.0), vec3<f32>(1.0)), i.color.a);
}

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world: vec3<f32>,
    // 線の始まり(線の 1 つ目の端の値をそのまま使う)と、今の所の画面の上の位置
    @location(2) @interpolate(flat) start: vec2<f32>,
    @location(3) @interpolate(linear) at: vec2<f32>,
};

@vertex
fn vs_line(@location(0) p: vec3<f32>, @location(1) c: vec4<f32>) -> LineOut {
    var o: LineOut;
    o.pos = clip(p, u.misc.y);
    o.color = c;
    o.world = p;
    o.start = screen(o.pos);
    o.at = o.start;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    if (cut(i.world)) {
        discard;
    }
    return i.color;
}

// 格子と軸・透かす道具の稜線(断面でも切らない)
@fragment
fn fs_guide(i: LineOut) -> @location(0) vec4<f32> {
    return i.color;
}

// 隠れた辺: 線の始まりからの画面の上の長さで点線に刻み、濃さを下げる
@fragment
fn fs_hidden(i: LineOut) -> @location(0) vec4<f32> {
    if (cut(i.world)) {
        discard;
    }
    let d = distance(i.at, i.start);
    if (d - floor(d / u.extra.w) * u.extra.w > u.extra.z) {
        discard;
    }
    return vec4<f32>(i.color.rgb, i.color.a * u.extra.y);
}

// 稜線は幅のある四角で描く(GPU の線の太さは装置で違い、MSAA の DirectX 12 は 1.4 画素で描くため)。
// 1 本の線を 6 頂点の 2 枚の三角形にし、画面の上で線と直角に広げる。u.misc.w は描く先の画素での太さ
struct WideOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world: vec3<f32>,
    @location(2) @interpolate(flat) start: vec2<f32>,
    @location(3) @interpolate(linear) at: vec2<f32>,
    // 線の真ん中からの距離(描く先の画素)
    @location(4) @interpolate(linear) side: f32,
};

@vertex
fn vs_wide(
    @builtin(vertex_index) k: u32,
    @location(0) p0: vec3<f32>,
    @location(1) c: vec4<f32>,
    @location(4) p1: vec3<f32>,
) -> WideOut {
    let a = clip(p0, u.misc.y);
    let b = clip(p1, u.misc.y);
    let px = u.proj.zw * u.misc.z;
    let sa = a.xy / max(a.w, 1e-6) * px;
    let sb = b.xy / max(b.w, 1e-6) * px;
    let len = length(sb - sa);
    let dir = select(vec2<f32>(1.0, 0.0), (sb - sa) / len, len > 1e-6);
    let nrm = vec2<f32>(-dir.y, dir.x);
    // (端 0 = 始まり・1 = 終わり, 側 −1・+1)
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let q = corners[k];
    let end = q.x > 0.5;
    let base = select(a, b, end);
    // 縁のなめらかさの分だけ 1 画素広げ、端は半画素伸ばしてつなぎ目を埋める
    let half = 0.5 * u.misc.w + 1.0;
    let off = nrm * q.y * half + dir * (q.x * 2.0 - 1.0) * 0.5;
    var o: WideOut;
    o.pos = vec4<f32>(base.xy + off / px * base.w, base.zw);
    o.color = c;
    o.world = select(p0, p1, end);
    o.start = screen(a);
    o.at = screen(base);
    o.side = q.y * half;
    return o;
}

// 太さの中にかかる割合(縁は 1 画素でなめらかに落とす)
fn wide_cover(side: f32) -> f32 {
    return clamp(0.5 * u.misc.w + 0.5 - abs(side), 0.0, 1.0);
}

@fragment
fn fs_wide(i: WideOut) -> @location(0) vec4<f32> {
    if (cut(i.world)) {
        discard;
    }
    return vec4<f32>(i.color.rgb, i.color.a * wide_cover(i.side));
}

// 隠れた辺(fs_hidden と同じ点線)
@fragment
fn fs_wide_hidden(i: WideOut) -> @location(0) vec4<f32> {
    if (cut(i.world)) {
        discard;
    }
    let d = distance(i.at, i.start);
    if (d - floor(d / u.extra.w) * u.extra.w > u.extra.z) {
        discard;
    }
    return vec4<f32>(i.color.rgb, i.color.a * u.extra.y * wide_cover(i.side));
}

// 入り隅の陰の前の段: 見えている面の位置を残す。断面で切り取った所は描かない
struct PosFaceOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_pos_face(@location(0) p: vec3<f32>) -> PosFaceOut {
    var o: PosFaceOut;
    o.pos = clip(p, 0.0);
    o.world = p;
    return o;
}

@fragment
fn fs_ao_pos(i: PosFaceOut) -> @location(0) vec4<f32> {
    if (cut(i.world)) {
        discard;
    }
    return vec4<f32>(i.world, 1.0);
}
