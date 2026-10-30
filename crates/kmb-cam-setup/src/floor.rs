//! 加工が削る領域を選んだ面・輪郭から作る(底の面、届かない所、角の丸め)と、その欄。

use super::*;

/// 平らな底の面から、削る領域と底の高さを作る。島でない内周は外す。
/// 外周の外に部品の壁がない開いた面なら、ストックの縁から `margin` だけ外まで広げる。
pub fn floor_area(p: &Prepared, face: &str, margin: f64) -> Result<(Area, f64), EvalError> {
    let f = p.model.flat(face).ok_or_else(|| {
        invalid(format!(
            "面 {face} が見つかりません(参照が切れたか、水平な平面ではありません)"
        ))
    })?;
    floor_face(p, f, margin)
}

fn floor_face(
    p: &Prepared,
    f: &kmb_cam_geom::FlatFace,
    margin: f64,
) -> Result<(Area, f64), EvalError> {
    if !f.up {
        return Err(invalid(format!("面 {} は上を向いていません", f.name)));
    }
    let holes = f
        .area
        .holes
        .iter()
        .filter(|h| {
            // 内周の中の 1 点で、部品が底より高いか調べる
            inner_point(h).is_some_and(|q| p.model.top_at(q).is_some_and(|z| z > f.z + 1e-3))
        })
        .cloned()
        .collect();
    // 外周の外側に部品の壁がなければ(上面のような開いた面なら)、ストックの縁まで削る
    let nested = p
        .info
        .sections
        .as_ref()
        .is_some_and(|s| s.kind == crate::sections::SectionKind::Sheets);
    let outer = if nested || has_wall(&p.model, &f.area.outer, f.z) {
        f.area.outer.clone()
    } else {
        let (min, max) = (p.info.stock_min, p.info.stock_max);
        let e = margin;
        Loop::from_points(&[
            [min[0] - e, min[1] - e],
            [max[0] + e, min[1] - e],
            [max[0] + e, max[1] + e],
            [min[0] - e, max[1] + e],
        ])
    };
    Ok((Area { outer, holes }, f.z))
}

/// 領域の縁(外周の外と、島の側)のすぐ外での、部品のいちばん高い所。縁に部品の壁がなければ `None`。
/// V やテーパーの工具が壁に当たらないかを、壁の上端からの深さで調べるのに使う。
pub fn wall_top(model: &MachiningModel, area: &Area) -> Option<f64> {
    let mut top: Option<f64> = None;
    for l in std::iter::once(&area.outer).chain(&area.holes) {
        let pts = l.flatten(1e-3);
        let n = pts.len();
        let ccw = l.signed_area() > 0.0;
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let len = d[0].hypot(d[1]);
            if len < 1e-6 {
                continue;
            }
            // 反時計回りの輪郭なら、領域の外は右側
            let s = if ccw { 0.05 } else { -0.05 };
            for t in [0.0, 0.5] {
                let q = [
                    a[0] + d[0] * t + d[1] / len * s,
                    a[1] + d[1] * t - d[0] / len * s,
                ];
                if let Some(z) = model.top_at(q) {
                    top = Some(top.map_or(z, |w: f64| w.max(z)));
                }
            }
        }
    }
    top
}

/// 外周のすぐ外に、底より高い部品があるか。
fn has_wall(model: &MachiningModel, outer: &Loop, z: f64) -> bool {
    let pts = outer.flatten(1e-3);
    let n = pts.len();
    let ccw = outer.signed_area() > 0.0;
    (0..n).any(|i| {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]);
        if len < 1e-6 {
            return false;
        }
        // 反時計回りの輪郭なら、外は右側
        let s = if ccw { 0.05 } else { -0.05 };
        let q = [
            (a[0] + b[0]) / 2.0 + d[1] / len * s,
            (a[1] + b[1]) / 2.0 - d[0] / len * s,
        ];
        model.top_at(q).is_some_and(|t| t > z + 1e-3)
    })
}

/// 輪郭の内側の点(輪郭の近くで、内側にあるもの)。輪郭は網の境目から円弧に戻したもので、
/// 網の面は円弧の内側に最大で網の許容差だけ欠ける。輪郭のすぐ内側の点はその欠けに
/// 落ちることがあるので、許容差より奥(0.05 mm)を先に試す。
fn inner_point(l: &Loop) -> Option<P2> {
    let pts = l.flatten(1e-3);
    let n = pts.len();
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]);
        if len < 1e-6 {
            continue;
        }
        for s in [0.05, -0.05, 1e-3, -1e-3] {
            let q = [mid[0] - d[1] / len * s, mid[1] + d[0] / len * s];
            if l.contains(q) {
                return Some(q);
            }
        }
    }
    None
}

/// 工具の半径より小さい内側の角に削り残る、上から見た面積(壁から `leave` 残す時)。
/// 工具の中心が届く所を工具の半径だけ広げた領域が、削れる所。
pub fn unreachable_area(area: &Area, r: f64, leave: f64) -> f64 {
    let reach: f64 = offset_area(area, r + leave)
        .iter()
        .flat_map(|a| offset_area(a, -(r + leave)))
        .map(|a| a.area())
        .sum();
    offset_area(area, leave)
        .iter()
        .map(|a| a.area())
        .sum::<f64>()
        - reach
}

/// 壁(径方向)と底(軸方向)に残す厚さ。`floor` が空なら底も壁と同じ
/// (壁と底に 1 つの厚さを残していた文書は、両方に同じ値として読む)。
pub fn leaves(
    ctx: &EvalCtx<'_>,
    wall: &str,
    floor: &Option<String>,
) -> Result<(f64, f64), EvalError> {
    let w = length(ctx, wall, "壁の残し量")?;
    let f = match floor {
        Some(src) => length(ctx, src, "底の残し量")?,
        None => w,
    };
    Ok((w, f))
}

/// 壁と底に残す厚さの欄(`leave` と `leave_floor`)。
pub fn leave_fields() -> [Field; 2] {
    [
        Field::expr("leave", "壁の残し量")
            .length()
            .help("径方向。底の欄が空なら、底にも同じ厚さを残す"),
        Field::opt_expr("leave_floor", "底の残し量")
            .length()
            .help("軸方向。空なら壁と同じ。例: 壁に 0.3 mm 残して荒削りし、底は 0 mm で仕上げる"),
    ]
}

/// 最後の段の厚さの欄(`last_step`。[`Prepared::levels`] が使う)。
pub fn last_step_field() -> Field {
    Field::opt_expr("last_step", "仕上げの切込み")
        .length()
        .help(
            "例: 0.2 mm。最後の段だけこの厚さで薄く削り、底をきれいにする。空なら段を等しく分ける",
        )
}

/// 最小の切削半径の欄(`min_radius`)。
pub fn min_radius_field() -> Field {
    Field::opt_expr("min_radius", "最小切削半径").length().help(
        "例: 1 mm。経路の内側の角をこの半径以上に丸め、工具が角に食い込む動きを減らす。\
         角には材料が残る(壁の仕上げや残り削りで取る)。空なら丸めない",
    )
}

/// 長さの欄(空なら `None`)。0 以上でなければ誤り。
pub fn opt_length(
    ctx: &EvalCtx<'_>,
    src: &Option<String>,
    what: &str,
) -> Result<Option<f64>, EvalError> {
    let Some(src) = src else {
        return Ok(None);
    };
    let v = length(ctx, src, what)?;
    if !(v >= 0.0 && v.is_finite()) {
        return Err(invalid(format!("{what}は 0 以上にしてください")));
    }
    Ok(Some(v))
}

/// 工具の中心が回る領域の出っ張った角を、半径 `radius` 以上に丸める(内側へ寄せてから同じだけ
/// 外へ戻す)。経路の角が丸くなり、壁の内側の角には材料が残る。寄せると消えてしまう細い領域は
/// 削り忘れないよう、そのまま残す。
pub fn round_corners(areas: Vec<Area>, radius: f64) -> Vec<Area> {
    if radius <= 1e-6 {
        return areas;
    }
    let mut out = Vec::new();
    for a in areas {
        let opened: Vec<Area> = offset_area(&a, radius)
            .iter()
            .flat_map(|x| offset_area(x, -radius))
            .collect();
        if opened.is_empty() {
            out.push(a);
        } else {
            out.extend(opened);
        }
    }
    out
}

/// スケッチの閉じた輪郭(出力 `profile`)の領域を、機械に置いた向きの作業座標系で返す。
/// 高さは輪郭の平面の高さ。平面は機械に置いた向きで水平でなければならない。
pub fn profile_areas(
    ctx: &EvalCtx<'_>,
    p: &Prepared,
    id: FeatureId,
) -> Result<(Vec<Area>, f64), EvalError> {
    let profile = kmb_core::Profile::from_value(ctx.output(id, "profile")?)
        .ok_or_else(|| invalid(format!("{id} は閉じた輪郭を出しません")))?;
    if profile.regions.is_empty() {
        return Err(invalid(
            "スケッチに閉じた輪郭がありません(削る領域を閉じた線で描いてください)",
        ));
    }
    let info = &p.info;
    let place = |q: P3| {
        let w = info.place(q);
        sub(w, info.origin)
    };
    let mut z: Option<f64> = None;
    let mut areas = Vec::new();
    for r in &profile.regions {
        let mut loops = Vec::new();
        for l in std::iter::once(&r.outer).chain(&r.holes) {
            let mut pts: Vec<P2> = Vec::new();
            for q in l.polyline() {
                let w = place(profile.plane.to_world(q));
                match z {
                    None => z = Some(w[2]),
                    Some(z0) if (w[2] - z0).abs() > 1e-4 => {
                        return Err(invalid(
                            "スケッチの平面が、機械に置いた向きで水平ではありません(上か下を向いた面に描いてください)",
                        ));
                    }
                    _ => {}
                }
                pts.push([w[0], w[1]]);
            }
            // 円は折れ線で来るので、円弧に戻す
            loops.push(kmb_cam_geom::fit_arcs(&pts, 0.005));
        }
        let outer = loops.remove(0);
        areas.push(
            Area {
                outer,
                holes: loops,
            }
            .normalized(),
        );
    }
    Ok((areas, z.expect("輪郭がある")))
}

/// ポケットと負荷制御粗取りで、削る領域の選び方。
#[derive(Debug, Clone, Copy)]
pub struct FloorPick<'a> {
    /// 底の面(空なら使わない)
    pub face: &'a str,
    /// ほかの底の面
    pub faces: &'a [String],
    /// 閉じた輪郭を出すスケッチ
    pub profile: Option<FeatureId>,
    /// スケッチの平面から底までの深さ。空なら平面の高さが底
    pub depth: &'a Option<String>,
}

/// 削る領域の 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Floor {
    pub area: Area,
    /// 底の高さ
    pub z: f64,
    /// スケッチの輪郭から作った領域なら、スケッチの平面の高さ(部品の形にない所を削る)
    pub sketch_top: Option<f64>,
}

/// 選んだ底の面とスケッチの輪郭から、削る領域を集める(面の順、スケッチの輪郭は後)。
/// 開いた面は、ストックの縁から `margin` だけ外まで広げる([`floor_area`])。
pub fn floors(
    ctx: &EvalCtx<'_>,
    p: &Prepared,
    pick: FloorPick<'_>,
    margin: f64,
) -> Result<Vec<Floor>, EvalError> {
    let mut names: Vec<&str> = Vec::new();
    for f in std::iter::once(pick.face).chain(pick.faces.iter().map(String::as_str)) {
        if !f.is_empty() && !names.contains(&f) {
            names.push(f);
        }
    }
    let mut out = Vec::new();
    for f in names {
        let faces = p.model.flats_named(f);
        if faces.is_empty() {
            return Err(invalid(format!(
                "面 {f} が見つかりません(参照が切れたか、水平な平面ではありません)"
            )));
        }
        for face in faces {
            let (area, z) = floor_face(p, face, margin)?;
            out.push(Floor {
                area,
                z,
                sketch_top: None,
            });
        }
    }
    if let Some(id) = pick.profile {
        let (areas, plane) = profile_areas(ctx, p, id)?;
        let depth = opt_length(ctx, pick.depth, "スケッチからの深さ")?;
        let z = plane - depth.unwrap_or(0.0);
        if z >= p.info.stock_max[2] - 1e-6 {
            return Err(invalid(
                "スケッチの平面がストックの上面より下にないので、深さの欄で削る深さを決めてください",
            ));
        }
        out.extend(areas.into_iter().map(|area| Floor {
            area,
            z,
            sketch_top: Some(plane),
        }));
    }
    if out.is_empty() {
        return Err(invalid(
            "削る所を選んでいません(底の面か、閉じた輪郭のあるスケッチを選んでください)",
        ));
    }
    Ok(out)
}

/// 選んだ領域の依存(スケッチの輪郭と、深さの式)。
pub fn floor_inputs(pick: FloorPick<'_>) -> Result<Vec<InputRef>, EvalError> {
    let mut v: Vec<InputRef> = pick
        .profile
        .map(|id| output(id, "profile"))
        .into_iter()
        .collect();
    v.extend(expr_refs(pick.depth)?);
    Ok(v)
}

/// 選んだ領域の欄(底の面・ほかの底の面・スケッチ・深さ)。
pub fn floor_fields() -> [Field; 4] {
    [
        Field::info("face", "底の面"),
        Field::info("faces", "ほかの底の面").help(
            "3D のビューで Shift・Ctrl+クリックで面を選び、「選んだ面を足す」「選んだ面を外す」で変える",
        ),
        Field::opt_output("profile", "スケッチの輪郭", "profile")
            .help("閉じた輪郭の内側も削る。深さが空なら、スケッチの平面の高さまで削る"),
        Field::opt_expr("depth", "スケッチからの深さ").length()
            .help("スケッチの平面から底まで(例: 3 mm)。空ならスケッチの平面が底"),
    ]
}
