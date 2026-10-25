//! スケッチの輪郭と、線で分けた領域を選ぶ共通部(押し出し・回転が使う)。
use kmb_core::{Context, FeatureId, Gesture, Handle, HandleChoice, Profile, ProfileFaces};

/// 機能 `id` の閉じた輪郭(なければ `None`)。
pub fn profile_of(cx: &Context<'_>, id: FeatureId) -> Option<Profile> {
    cx.output(id, "profile")
        .and_then(Profile::from_value)
        .filter(|p| !p.regions.is_empty())
}

/// 機能 `id` の、1 つずつ選べる領域(名前と領域)。線で分けた領域を出さない機能(文字など)は、
/// 輪郭の全部を名前なしの 1 つにする。
pub fn faces_of(cx: &Context<'_>, id: FeatureId) -> Vec<(Option<String>, Profile)> {
    let Some(p) = profile_of(cx, id) else {
        return Vec::new();
    };
    match cx.output(id, "faces").and_then(ProfileFaces::from_value) {
        Some(f) if !f.faces.is_empty() => f
            .faces
            .into_iter()
            .map(|face| {
                (
                    Some(face.key),
                    Profile {
                        plane: f.plane,
                        regions: vec![face.region],
                    },
                )
            })
            .collect(),
        _ => vec![(None, p)],
    }
}

/// 選んでいる機能が、閉じた輪郭のあるスケッチなら、その ID。
pub fn selected_profile(cx: &Context<'_>) -> Option<FeatureId> {
    let id = cx.selected?;
    profile_of(cx, id).map(|_| id)
}

/// 領域を選ぶ手順で候補にするスケッチ: 選んでいるスケッチ、なければ輪郭のある機能の全部。
pub fn region_sketches(cx: &Context<'_>) -> Vec<FeatureId> {
    match selected_profile(cx) {
        Some(id) => vec![id],
        None => candidates(cx),
    }
}

/// 輪郭のある機能(古い順)。
pub fn candidates(cx: &Context<'_>) -> Vec<FeatureId> {
    cx.candidates("profile")
        .into_iter()
        .map(FeatureId)
        .filter(|id| has_profile(cx, *id))
        .collect()
}

/// 機能 `id` に領域のある輪郭があるか([`profile_of`] が `Some` か。輪郭を読まずに見る。
/// 操作の一覧は 1 コマごとに全部の機能を見るので、機能が多いと読むだけで重い。10-24 v10)。
pub fn has_profile(cx: &Context<'_>, id: FeatureId) -> bool {
    match cx.output(id, "profile") {
        Some(kmb_core::Value::Custom { type_key, data }) if type_key == kmb_core::PROFILE_TYPE => {
            data.get("regions")
                .and_then(|r| r.as_array())
                .is_some_and(|r| !r.is_empty())
        }
        _ => false,
    }
}

/// 指定の `regions`(領域を選ぶ手順の値 `[{"profile": 3, "key": "e2.0+e7"}]`、CLI・MCP は
/// `profile` と名前の一覧 `["e2.0+e7"]` でもよい)か `profile` で決めたスケッチと領域の名前
/// (空なら輪郭の全部)。どちらも無ければ `None`。
pub fn chosen_in_args(cx: &Context<'_>) -> Option<(FeatureId, Vec<String>)> {
    let given = cx.arg("profile").and_then(|v| v.as_u64()).map(FeatureId);
    if let Some(picks) = cx.arg("regions").and_then(|v| v.as_array())
        && !picks.is_empty()
    {
        let mut sketch: Option<FeatureId> = None;
        let mut keys = Vec::new();
        let mut whole = false;
        for p in picks {
            let (id, key) = match p {
                serde_json::Value::String(k) => (given?, Some(k.clone())),
                serde_json::Value::Object(o) => (
                    FeatureId(o.get("profile")?.as_u64()?),
                    o.get("key").and_then(|k| k.as_str()).map(String::from),
                ),
                _ => return None,
            };
            // 違うスケッチの領域は使わない(画面は別のスケッチを選ぶと前の選びを外す)
            if *sketch.get_or_insert(id) != id {
                continue;
            }
            match key {
                Some(k) => keys.push(k),
                None => whole = true,
            }
        }
        let id = sketch?;
        profile_of(cx, id)?;
        // 領域が 1 つだけのスケッチは名前を覚えない(線を変えても全体を使う)
        let whole = whole || faces_of(cx, id).len() <= 1;
        return Some((id, if whole { Vec::new() } else { keys }));
    }
    let id = given?;
    profile_of(cx, id).map(|_| (id, Vec::new()))
}

/// 選んだ領域を合わせた輪郭(名前が空なら輪郭の全部)。
pub fn chosen_profile(
    cx: &Context<'_>,
    sketch: FeatureId,
    keys: &[String],
) -> Result<Profile, String> {
    let none = || "選んだスケッチに閉じた輪郭がありません".to_string();
    if keys.is_empty() {
        return profile_of(cx, sketch).ok_or_else(none);
    }
    cx.output(sketch, "faces")
        .and_then(ProfileFaces::from_value)
        .ok_or_else(none)?
        .profile(keys)
}

/// 輪郭の線(世界座標、閉じた線)。
pub fn outline(p: &Profile) -> Vec<Vec<[f64; 3]>> {
    p.regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .map(|l| {
            let mut v: Vec<[f64; 3]> = l.polyline().iter().map(|q| p.plane.to_world(*q)).collect();
            if let Some(f) = v.first().copied() {
                v.push(f);
            }
            v
        })
        .collect()
}

/// 輪郭の面の重心(穴を引いた面積で重み付け。面積が無ければ点の平均)。
pub fn centroid(p: &Profile) -> [f64; 3] {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    let mut pts = Vec::new();
    for r in &p.regions {
        let loops = std::iter::once((&r.outer, 1.0)).chain(r.holes.iter().map(|h| (h, -1.0)));
        for (l, sign) in loops {
            let q = l.polyline();
            let (mut s, mut sx, mut sy) = (0.0, 0.0, 0.0);
            for i in 0..q.len() {
                let (u, v) = (q[i], q[(i + 1) % q.len()]);
                let k = u[0] * v[1] - v[0] * u[1];
                s += k;
                sx += (u[0] + v[0]) * k;
                sy += (u[1] + v[1]) * k;
            }
            pts.extend(q);
            if s.abs() > 1e-12 {
                // 向き(左回り・右回り)によらず、面積の大きさで重みを付ける
                let w = sign * (s / 2.0).abs();
                a += w;
                cx += w * sx / (3.0 * s);
                cy += w * sy / (3.0 * s);
            }
        }
    }
    let c = if a.abs() > 1e-12 {
        [cx / a, cy / a]
    } else {
        let n = pts.len().max(1) as f64;
        let s = pts.iter().fold([0.0; 2], |s, q| [s[0] + q[0], s[1] + q[1]]);
        [s[0] / n, s[1] / n]
    };
    p.plane.to_world(c)
}

/// 領域を選ぶ手順の候補: スケッチ `sketches` の、線で分けた領域の全部
/// (値 `{"profile": スケッチ, "key": 領域の名前}`。スケッチごとに組を分ける)。
pub fn region_choices(cx: &Context<'_>, sketches: &[FeatureId]) -> Vec<HandleChoice> {
    let mut choices = Vec::new();
    for &id in sketches {
        let name = cx
            .data
            .feature(id)
            .map_or_else(|| id.to_string(), |f| f.name.clone());
        let faces = faces_of(cx, id);
        let many = faces.len() > 1;
        for (k, (key, p)) in faces.into_iter().enumerate() {
            choices.push(HandleChoice {
                value: serde_json::json!({"profile": id.0, "key": key}),
                label: if many {
                    format!("{name} の領域 {}", k + 1)
                } else {
                    name.clone()
                },
                outline: outline(&p),
                group: id.0,
                ..Default::default()
            });
        }
    }
    choices
}

/// 領域 `r` の中の点(平面の座標)。外形の重心が穴の中や外形の外なら、外形の頂点の近くを探す。
pub fn inner_point(r: &kmb_core::kernel::Region) -> Option<kmb_core::kernel::P2> {
    use kmb_core::kernel::point_in_polygon;
    let outer = r.outer.polyline();
    let holes: Vec<_> = r.holes.iter().map(|h| h.polyline()).collect();
    let ok =
        |p: [f64; 2]| point_in_polygon(p, &outer) && !holes.iter().any(|h| point_in_polygon(p, h));
    let n = outer.len().max(1) as f64;
    let c = outer
        .iter()
        .fold([0.0; 2], |a, q| [a[0] + q[0] / n, a[1] + q[1] / n]);
    // 重心から各頂点へ向かう線の上を、重心の側から順に試す
    let mut cands = vec![c];
    for q in &outer {
        for t in [0.5, 0.9, 0.98] {
            cands.push([c[0] + (q[0] - c[0]) * t, c[1] + (q[1] - c[1]) * t]);
        }
    }
    cands.into_iter().find(|p| ok(*p))
}

/// 輪郭の全部 `whole` が、領域 `part` を含むか(領域の中の点が、全部の外形の中で穴の外にある)。
fn covers(whole: &Profile, part: &Profile) -> bool {
    use kmb_core::kernel::point_in_polygon;
    part.regions.iter().filter_map(inner_point).all(|p| {
        whole.regions.iter().any(|w| {
            point_in_polygon(p, &w.outer.polyline())
                && !w.holes.iter().any(|h| point_in_polygon(p, &h.polyline()))
        })
    })
}

/// スケッチ `sketch` の全部を使う時に押し出される領域の名前(全部の輪郭の穴に当たる領域は入れない)。
fn whole_keys(cx: &Context<'_>, sketch: FeatureId) -> Vec<String> {
    let Some(whole) = profile_of(cx, sketch) else {
        return Vec::new();
    };
    faces_of(cx, sketch)
        .into_iter()
        .filter(|(_, p)| covers(&whole, p))
        .filter_map(|(k, _)| k)
        .collect()
}

/// 作った後に領域を選び直す手順。候補は機能が使う
/// スケッチ `sketch` の領域で、今使っている領域 `keys`(空ならスケッチの全部が使う領域)を選んだまま始める。
pub fn repick_handle(cx: &Context<'_>, sketch: FeatureId, keys: &[String], label: &str) -> Handle {
    let choices = region_choices(cx, &[sketch]);
    let now = if keys.is_empty() {
        whole_keys(cx, sketch)
    } else {
        keys.to_vec()
    };
    let key_of = |c: &HandleChoice| {
        c.value
            .get("key")
            .and_then(|k| k.as_str())
            .map(String::from)
    };
    let mut picked: Vec<serde_json::Value> = choices
        .iter()
        .filter(|c| key_of(c).is_some_and(|k| now.contains(&k)))
        .map(|c| c.value.clone())
        .collect();
    // 線を変えてスケッチから無くなった領域は、囲む線がいちばん近い領域を選んでおく(黙って付け替えず、
    // 選び直しの手順で見せて人が決める。採用分 10-20 f27)
    let keys: Vec<String> = choices.iter().filter_map(key_of).collect();
    let mut stand_ins = Vec::new();
    for lost in now.iter().filter(|k| !keys.contains(k)) {
        if let Some(c) = nearest_region(lost, &choices)
            && !picked.contains(&c.value)
        {
            picked.push(c.value.clone());
            stand_ins.push(c.label.clone());
        }
    }
    let label = if stand_ins.is_empty() {
        label.to_string()
    } else {
        format!(
            "スケッチの線が変わり、前の領域がありません。線の近い領域({})を選んでおいたので、違えばクリックで直す。{label}",
            stand_ins.join("、")
        )
    };
    Handle {
        choices,
        picked,
        ..Handle::new("regions", &label, Gesture::Regions)
    }
}

/// 無くなった領域 `lost`(囲む区間の名前を `+` で並べた名前)に、囲む線がいちばん近い候補。
/// 同じ区間を 2、同じ線(区間の番号 `.N` を除いた名前)を 1 と数え、多い物。同じ線が 1 本も無ければ無し。
fn nearest_region<'a>(lost: &str, choices: &'a [HandleChoice]) -> Option<&'a HandleChoice> {
    let line = |seg: &str| seg.split('.').next().unwrap_or(seg).to_string();
    let segs: Vec<&str> = lost.split('+').collect();
    let lines: Vec<String> = segs.iter().map(|s| line(s)).collect();
    choices
        .iter()
        .filter_map(|c| {
            let key = c.value.get("key")?.as_str()?;
            let score: usize = key
                .split('+')
                .map(|s| {
                    if segs.contains(&s) {
                        2
                    } else if lines.contains(&line(s)) {
                        1
                    } else {
                        0
                    }
                })
                .sum();
            (score > 0).then_some((score, c))
        })
        // 同じ数なら先の候補(max_by_key は後ろを返すので逆に数える)
        .rev()
        .max_by_key(|(n, _)| *n)
        .map(|(_, c)| c)
}

/// 領域を選び直せるか: 機能が使うスケッチ `sketch` に領域が 2 つ以上ある時。
pub fn can_repick(cx: &Context<'_>, sketch: FeatureId) -> bool {
    faces_of(cx, sketch).len() > 1
}

/// 選び直した領域(引数 `regions`)の名前。スケッチの全部と同じ領域なら空(スケッチの全部を使う
/// まま。線を足しても全部を使う)。
pub fn repicked(cx: &Context<'_>, sketch: FeatureId) -> Result<Vec<String>, String> {
    match chosen_in_args(cx) {
        Some((id, mut keys)) if id == sketch => {
            let mut whole = whole_keys(cx, sketch);
            keys.sort();
            whole.sort();
            Ok(if keys == whole { Vec::new() } else { keys })
        }
        Some(_) => Err("機能が使うスケッチの領域を選んでください".into()),
        None => Err("領域を 1 つ以上クリックしてください".into()),
    }
}

/// [`chosen_in_args`] が読む引数(押し出す・回す領域)。
pub fn args() -> Vec<kmb_core::Arg> {
    use kmb_core::Arg;
    vec![
        Arg::feature("profile", "使うスケッチ(機能の番号)"),
        Arg::list(
            "regions",
            "領域の並び。手順の値 [{\"profile\": 3, \"key\": \"e2.0+e7\"}] か、profile と一緒に名前の並び [\"e2.0+e7\"]。省くと輪郭の全部",
        ),
    ]
}
