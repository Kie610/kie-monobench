//! 視点。透視投影か平行投影。向きは画面の視点と同じ決め方(ヨーは Z のまわり、ピッチは上下)。

use crate::math::{V3, add, cross, dot, madd, normalize, scale, sub};

/// 透視投影の既定の縦の画角(度)。
pub const FOV_DEG: f32 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub eye: V3,
    /// 画面の右、上、奥(見る向き)。互いに直交し、長さ 1
    pub right: V3,
    pub up: V3,
    pub forward: V3,
    /// 透視投影の縦の画角(度)。0 なら平行投影
    pub fov_y_deg: f32,
    /// 平行投影で、画面の縦に入る長さ(mm)
    pub ortho_height: f32,
    /// 画面の真ん中のずれ(-1〜1 の画面の位置で。画面のビューを平行に動かした分)
    pub shift: [f32; 2],
    /// 焦点の距離(視点から見る向きに mm。画面のビューでは回す中心まで)。0 なら分からない
    pub focus: f32,
    /// レンズの穴の半径(mm、被写界深度)。0 ならぼかさない(透視投影の時だけ効く)
    pub aperture: f32,
}

/// 35 mm 判(縦 24 mm)の焦点距離(mm)から縦の画角(度)。
pub fn fov_from_focal(focal_mm: f32) -> f32 {
    2.0 * (12.0 / focal_mm.max(1.0)).atan().to_degrees()
}

/// ヨー・ピッチ(度)の(右、上、視点へ向かう向き)。画面のカメラと同じ式。
pub fn basis(yaw_deg: f32, pitch_deg: f32) -> (V3, V3, V3) {
    basis_rad(yaw_deg.to_radians(), pitch_deg.to_radians())
}

/// [`basis`] の角度をラジアンで受ける形(3D のビューのカメラが使う)。
pub fn basis_rad(yaw: f32, pitch: f32) -> (V3, V3, V3) {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let eye = [cp * cy, cp * sy, sp];
    let forward = eye.map(|v| -v);
    let right = normalize(cross(forward, [0.0, 0.0, 1.0]));
    let up = cross(right, forward);
    (right, up, eye)
}

impl Camera {
    /// 視点の位置と向きから(`right`・`up` は画面の右と上、`toward_eye` は注視点から視点へ)。
    pub fn from_basis(
        eye: V3,
        right: V3,
        up: V3,
        toward_eye: V3,
        fov_y_deg: f32,
        ortho_height: f32,
    ) -> Camera {
        Camera {
            eye,
            right: normalize(right),
            up: normalize(up),
            forward: normalize(scale(toward_eye, -1.0)),
            fov_y_deg,
            ortho_height,
            shift: [0.0; 2],
            focus: 0.0,
            aperture: 0.0,
        }
    }

    /// 箱 `min`〜`max` が全部入るように、向き(ヨー・ピッチ、度)から視点を決める。`aspect` は横÷縦。
    /// 箱を包む球で合わせるので、回しても大きさが変わらない(ターンテーブル)。
    pub fn fit(
        min: V3,
        max: V3,
        yaw_deg: f32,
        pitch_deg: f32,
        fov_y_deg: f32,
        aspect: f32,
    ) -> Camera {
        let (right, up, toward) = basis(yaw_deg, pitch_deg);
        let center = scale(add(min, max), 0.5);
        let radius = (crate::math::len(sub(max, min)) * 0.5).max(1.0);
        let margin = 1.08;
        let aspect = aspect.max(0.05);
        if fov_y_deg > 0.0 {
            let half_y = (fov_y_deg.to_radians() / 2.0).tan();
            let half = half_y.min(half_y * aspect).atan();
            let dist = radius * margin / half.sin();
            let mut c = Camera::from_basis(
                madd(center, toward, dist),
                right,
                up,
                toward,
                fov_y_deg,
                0.0,
            );
            c.focus = dist;
            c
        } else {
            let h = 2.0 * radius * margin / aspect.min(1.0);
            let dist = radius * 4.0;
            Camera::from_basis(madd(center, toward, dist), right, up, toward, 0.0, h)
        }
    }

    /// 注視点から視点へ向かう向き。
    pub fn toward_eye(&self) -> V3 {
        scale(self.forward, -1.0)
    }

    pub fn perspective(&self) -> bool {
        self.fov_y_deg > 0.0
    }

    /// 画面の位置(`sx`・`sy` は -1〜1、`sy` は上が正)を通る光線(起点、長さ 1 の向き)。
    pub fn ray(&self, sx: f32, sy: f32, aspect: f32) -> (V3, V3) {
        let (sx, sy) = (sx - self.shift[0], sy - self.shift[1]);
        if self.perspective() {
            let t = (self.fov_y_deg.to_radians() / 2.0).tan();
            let d = add(
                add(self.forward, scale(self.right, sx * t * aspect)),
                scale(self.up, sy * t),
            );
            (self.eye, normalize(d))
        } else {
            let h = self.ortho_height / 2.0;
            let o = add(
                add(self.eye, scale(self.right, sx * h * aspect)),
                scale(self.up, sy * h),
            );
            (o, self.forward)
        }
    }

    /// 画面の枠 `view` の中の枠 `frame` だけを写す視点(どちらも画素で (中心 x, 中心 y, 幅, 高さ)、
    /// y は下向き)。視点の位置と向きは同じで、画角と真ん中のずれだけ替える(静止画の縦横比の枠)。
    /// 枠の中の点は、枠の外の視点で写したのと同じ所に写る。
    pub fn cropped(&self, view: [f32; 4], frame: [f32; 4]) -> Camera {
        let [vx, vy, vw, vh] = view.map(|v| v.max(1e-3));
        let [fx, fy, fw, fh] = frame.map(|v| v.max(1e-3));
        let k = fh / vh;
        let mut c = *self;
        if self.perspective() {
            let t = (self.fov_y_deg.to_radians() / 2.0).tan() * k;
            c.fov_y_deg = 2.0 * t.atan().to_degrees();
        } else {
            c.ortho_height = self.ortho_height * k;
        }
        c.shift = [
            (vx - fx + self.shift[0] * vw / 2.0) / (fw / 2.0),
            (fy - vy + self.shift[1] * vh / 2.0) / (fh / 2.0),
        ];
        c
    }

    /// [`Self::ray`] に被写界深度を足した光線。`lens` は 0〜1 の乱数 2 つで、レンズの穴の中の
    /// 1 点を選ぶ(薄いレンズ。焦点の距離の平面の点はぼけない)。穴が 0 か平行投影なら [`Self::ray`]。
    pub fn ray_lens(&self, sx: f32, sy: f32, aspect: f32, lens: [f32; 2]) -> (V3, V3) {
        let (o, d) = self.ray(sx, sy, aspect);
        if self.aperture <= 0.0 || self.focus <= 0.0 || !self.perspective() {
            return (o, d);
        }
        // 焦点の平面(見る向きに focus の所)で光線が通る点へ、レンズの穴の点から向ける
        let t = self.focus / dot(d, self.forward).max(1e-6);
        let p = madd(o, d, t);
        let r = self.aperture * lens[0].sqrt();
        let a = std::f32::consts::TAU * lens[1];
        let o2 = add(
            o,
            add(scale(self.right, r * a.cos()), scale(self.up, r * a.sin())),
        );
        (o2, normalize(sub(p, o2)))
    }

    /// 点 `p` を画面に写す((`sx`, `sy`, 奥行き))。奥行きは視点からの見る向きの距離。
    /// 視点より手前(後ろ)なら `None`。
    pub fn project(&self, p: V3, aspect: f32) -> Option<(f32, f32, f32)> {
        let d = sub(p, self.eye);
        let z = dot(d, self.forward);
        let x = dot(d, self.right);
        let y = dot(d, self.up);
        if self.perspective() {
            if z <= 1e-3 {
                return None;
            }
            let t = (self.fov_y_deg.to_radians() / 2.0).tan();
            Some((
                x / (z * t * aspect) + self.shift[0],
                y / (z * t) + self.shift[1],
                z,
            ))
        } else {
            let h = self.ortho_height / 2.0;
            Some((x / (h * aspect) + self.shift[0], y / h + self.shift[1], z))
        }
    }

    /// Z のまわりに `deg` 度回した視点(注視点 `center` を中心に。ターンテーブル)。
    pub fn orbit(&self, center: V3, deg: f32) -> Camera {
        let (s, c) = deg.to_radians().sin_cos();
        let rot = |v: V3| [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]];
        let eye = add(center, rot(sub(self.eye, center)));
        Camera {
            eye,
            right: rot(self.right),
            up: rot(self.up),
            forward: rot(self.forward),
            ..*self
        }
    }
}
