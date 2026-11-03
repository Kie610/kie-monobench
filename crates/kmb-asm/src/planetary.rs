//! 遊星の組をつなぐ。遊星歯車(太陽・腕・外輪と遊星の自転)と差動を、歯数から決まる 1 次式で
//! ジョイントの連動に結ぶ。
//!
//! 3 つの回転は、腕から見ると普通の歯車の列なので、歯数 z(太陽 s・外輪 r・遊星 p)と回転の角
//! (太陽 θs・腕 θc・外輪 θr)の間に `zs θs + zr θr = (zs + zr) θc` が成り立つ(どれか 2 つを
//! 決めると残りの 1 つが決まる)。遊星の腕に対する自転は `θp = −(zs / zp)(θs − θc)`
//! (外輪からなら `(zr / zp)(θr − θc)`)。差動は同じ式で、横の 2 つの歯車の歯数が同じ場合。
//!
//! タイムラインで 3 つのうちいちばん後ろのジョイントが、前の 2 つに連動する(連動元 2 つの
//! 1 次式。[`crate::Joint::link_joint2`])。遊星のジョイントは腕に対して回る(腕の部品を土台にした)
//! 回転のジョイントで、腕と太陽(か外輪)より後ろに置く。係数は欄の式(`連動元の動き`・`この動き`)
//! に歯数の整数で書くので、後から歯数を直す時もジョイントの編集で直せる。

use crate::anim::drives;
use crate::joint::{DriveBy, JOINT, Joint, JointKind};
use kmb_core::kernel as k;
use kmb_core::{Context, Edit, FeatureId, Gesture, Handle, HandleChoice, Outcome};
use serde_json::json;

/// 操作の ID。
pub const PLANETARY_LINK: &str = "asm.planetary_link";

/// 遊星の組の中の役。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Member {
    /// 太陽歯車(差動は 1 つ目の横の歯車)
    Sun,
    /// 腕(キャリア。差動は箱)
    Carrier,
    /// 外輪(内歯車。差動は 2 つ目の横の歯車)
    Ring,
    /// 遊星の腕に対する自転
    Planet,
}

impl Member {
    fn label(self) -> &'static str {
        match self {
            Member::Sun => "太陽",
            Member::Carrier => "腕",
            Member::Ring => "外輪",
            Member::Planet => "遊星",
        }
    }
}

/// 遊星の組の歯数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Teeth {
    pub sun: i64,
    pub ring: i64,
    /// 遊星(遊星の自転をつながない時は要らない)
    pub planet: Option<i64>,
}

/// 1 次式の 1 つの項: 後ろの値 += 連動元の値 × `follow` ÷ `lead`(`lead` は正)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Term {
    pub from: Member,
    pub lead: i64,
    pub follow: i64,
}

impl Term {
    /// 係数(`follow` ÷ `lead`)。
    pub fn ratio(&self) -> f64 {
        self.follow as f64 / self.lead as f64
    }
}

/// `who` の回転を、ほかの 2 つ(`from` の順)から決める 1 次式の 2 つの項。`who` が遊星なら、`from` は
/// 腕と、太陽か外輪。歯数が 0 以下か、遊星の歯数が無いと誤り。
pub fn terms(who: Member, from: [Member; 2], t: Teeth) -> Result<[Term; 2], String> {
    if t.sun < 1 || t.ring < 1 || t.planet.is_some_and(|p| p < 1) {
        return Err("歯数は 1 以上にしてください".into());
    }
    let planet = who == Member::Planet;
    let ok = |m: Member| m != who && m != Member::Planet;
    if from[0] == from[1]
        || !from.iter().all(|m| ok(*m))
        || (planet && !from.contains(&Member::Carrier))
    {
        return Err(if planet {
            "遊星の自転は腕と、太陽か外輪から決めます".into()
        } else {
            format!("{}の連動元の組が合いません", who.label())
        });
    }
    if planet {
        // θp = −(zs / zp)(θs − θc) = (zr / zp)(θr − θc)
        let p = t.planet.ok_or("遊星の歯数(planet_teeth)がありません")?;
        let (other, k) = if from.contains(&Member::Sun) {
            (Member::Sun, -t.sun)
        } else {
            (Member::Ring, t.ring)
        };
        let one = |m: Member| Term {
            from: m,
            lead: p,
            follow: if m == other { k } else { -k },
        };
        return Ok([one(from[0]), one(from[1])]);
    }
    // 3 つの回転の式 zs θs + zr θr − (zs + zr) θc = 0 の係数
    let coef = |m: Member| match m {
        Member::Sun => t.sun,
        Member::Ring => t.ring,
        _ => -(t.sun + t.ring),
    };
    let a = coef(who);
    let term = |m: Member| Term {
        from: m,
        lead: a.abs(),
        follow: -coef(m) * a.signum(),
    };
    Ok([term(from[0]), term(from[1])])
}

/// つなぐ 1 つ: 役・後ろのジョイント・2 つの連動元と項。
type Link = (Member, FeatureId, [(FeatureId, Term); 2]);

/// 手順: 太陽・腕・外輪・遊星(省ける)のジョイントの軸の線をクリック。
pub fn handles(cx: &Context<'_>) -> Vec<Handle> {
    let list: Vec<_> = drives(cx)
        .into_iter()
        .filter(|(_, _, d)| !d.slide)
        .collect();
    let step = |key: &str, text: &str, optional: bool| {
        let mut h = Handle::new(key, text, Gesture::Choice);
        h.optional = optional;
        for (id, name, d) in &list {
            let half = k::scale(d.axis, 40.0);
            h.choices.push(HandleChoice {
                value: json!(id.0),
                label: name.clone(),
                outline: vec![vec![k::sub(d.origin, half), k::add(d.origin, half)]],
                group: 0,
                sized: false,
                color: Some([230, 140, 30]),
            });
        }
        h
    };
    vec![
        step(
            "sun",
            "太陽歯車の回転のジョイントの軸の線をクリック(差動は 1 つ目の横の歯車)",
            false,
        ),
        step(
            "carrier",
            "腕(キャリア)の回転のジョイントの軸の線をクリック(差動は箱)",
            false,
        ),
        step(
            "ring",
            "外輪の回転のジョイントの軸の線をクリック(差動は 2 つ目の横の歯車)",
            false,
        ),
        step(
            "planet",
            "遊星の自転のジョイント(腕に対して回る)の軸の線をクリック。つながないなら Enter",
            true,
        ),
    ]
}

/// 遊星の組をつなぐ。引数: `sun`・`carrier`・`ring`(回転のジョイント)、`planet`(省ける)、
/// 歯数 `sun_teeth`・`ring_teeth`(省くと太陽と同じ = 差動)・`planet_teeth`(遊星をつなぐ時)。
pub fn run(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let id = |key: &str| cx.arg(key).and_then(|v| v.as_u64()).map(FeatureId);
    let need = |key: &str, m: Member| {
        id(key).ok_or(format!("{}のジョイント({key})を選んでいません", m.label()))
    };
    let mut members = vec![
        (Member::Sun, need("sun", Member::Sun)?),
        (Member::Carrier, need("carrier", Member::Carrier)?),
        (Member::Ring, need("ring", Member::Ring)?),
    ];
    if let Some(p) = id("planet") {
        members.push((Member::Planet, p));
    }
    for (i, (_, a)) in members.iter().enumerate() {
        if members[..i].iter().any(|(_, b)| a == b) {
            return Err(format!("機能 {} を 2 つの役に選んでいます", a.0).into());
        }
    }
    let teeth = |key: &str| -> Result<Option<i64>, String> {
        match cx.arg(key) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(v) => v
                .as_i64()
                .filter(|n| *n >= 1)
                .map(Some)
                .ok_or(format!("歯数 {key} は 1 以上の整数で指定します")),
        }
    };
    let sun_teeth = teeth("sun_teeth")?.ok_or(
        "太陽の歯数(sun_teeth)を指定してください(例: run asm.planetary_link sun=… carrier=… ring=… sun_teeth=24 ring_teeth=72)",
    )?;
    let t = Teeth {
        sun: sun_teeth,
        ring: teeth("ring_teeth")?.unwrap_or(sun_teeth),
        planet: teeth("planet_teeth")?,
    };
    if members.len() == 4 && t.planet.is_none() {
        return Err("遊星をつなぐ時は遊星の歯数(planet_teeth)を指定してください".into());
    }
    let list = drives(cx);
    let name_of = |j: FeatureId| -> Result<String, String> {
        list.iter()
            .find(|(id, _, d)| *id == j && !d.slide)
            .map(|(_, n, _)| n.clone())
            .ok_or(format!("機能 {} は回転のジョイントではありません", j.0))
    };
    for (_, j) in &members {
        name_of(*j)?;
    }
    let at = |j: FeatureId| cx.data.index_of(j).unwrap_or(usize::MAX);
    // 3 つの回転のうち、タイムラインでいちばん後ろが前の 2 つに連動する
    let mut three: Vec<(Member, FeatureId)> = members[..3].to_vec();
    three.sort_by_key(|(_, j)| at(*j));
    let mut links: Vec<Link> = Vec::new();
    let (who, follower) = three[2];
    let [a, b] = terms(who, [three[0].0, three[1].0], t)?;
    links.push((who, follower, [(three[0].1, a), (three[1].1, b)]));
    if let Some(&(_, planet)) = members.get(3) {
        let find = |m: Member| members.iter().find(|(x, _)| *x == m).map(|(_, j)| *j);
        let carrier = find(Member::Carrier).ok_or("腕がありません")?;
        if at(carrier) > at(planet) {
            return Err("遊星のジョイントは、腕のジョイントよりタイムラインで後ろに置いてください(遊星は腕に乗って回ります)".into());
        }
        let other = [Member::Sun, Member::Ring]
            .into_iter()
            .filter_map(|m| find(m).map(|j| (m, j)))
            .find(|(_, j)| at(*j) < at(planet))
            .ok_or(
                "遊星のジョイントは、太陽か外輪のジョイントよりタイムラインで後ろに置いてください",
            )?;
        let [a, b] = terms(Member::Planet, [Member::Carrier, other.0], t)?;
        links.push((Member::Planet, planet, [(carrier, a), (other.1, b)]));
    }
    let mut edits = Vec::new();
    let mut said = Vec::new();
    for (who, follower, [(j1, t1), (j2, t2)]) in links {
        let entry = cx
            .data
            .feature(follower)
            .filter(|e| e.type_id == JOINT)
            .ok_or("ジョイントではありません")?;
        let mut j: Joint = serde_json::from_value(entry.data.clone()).map_err(|e| e.to_string())?;
        if j.kind != JointKind::Revolute {
            return Err(format!(
                "{} は回転のジョイントではないので連動できません",
                entry.name
            )
            .into());
        }
        // 連動元が、これに連動していれば輪になる
        for lead in [j1, j2] {
            if let Some(l) = cx
                .data
                .feature(lead)
                .and_then(|e| serde_json::from_value::<Joint>(e.data.clone()).ok())
                && (l.linked() == Some(follower) || l.linked2() == Some(follower))
            {
                return Err(
                    format!("{} はもう {} に連動しています", name_of(lead)?, entry.name).into(),
                );
            }
        }
        j.drive_by = DriveBy::Link;
        j.link_joint = Some(j1);
        j.link_lead = format!("{} deg", t1.lead);
        j.link_follow = format!("{} deg", t1.follow);
        j.link_flip = false;
        j.link_joint2 = Some(j2);
        j.link_lead2 = format!("{} deg", t2.lead);
        j.link_follow2 = format!("{} deg", t2.follow);
        said.push(format!(
            "{}({})を {}・{} に連動",
            who.label(),
            entry.name,
            t1.from.label(),
            t2.from.label()
        ));
        edits.push(Edit::SetData {
            id: follower,
            data: serde_json::to_value(&j).map_err(|e| e.to_string())?,
        });
    }
    let mut label = format!(
        "遊星の組をつなぐ(歯数 太陽 {}・外輪 {}{}。{})",
        t.sun,
        t.ring,
        t.planet.map_or(String::new(), |p| format!("・遊星 {p}")),
        said.join("、")
    );
    // 平らな遊星歯車は外輪 = 太陽 + 2 × 遊星(差動・転位した歯車は別)
    if let Some(p) = t.planet
        && t.ring != t.sun
        && t.ring != t.sun + 2 * p
    {
        label.push_str("。注意: 外輪の歯数が 太陽 + 2 × 遊星 と違います");
        log::warn!("{label}");
    } else {
        log::info!("{label}");
    }
    Ok(Outcome::Change { label, edits })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Teeth = Teeth {
        sun: 24,
        ring: 72,
        planet: Some(24),
    };

    /// 1 次式の値(連動元の値は `of` で)。
    fn eval(terms: [Term; 2], of: impl Fn(Member) -> f64) -> f64 {
        terms.iter().map(|t| of(t.from) * t.ratio()).sum()
    }

    /// 外輪を止めると、太陽 4 回りで腕が 1 回り(1 + zr / zs = 4)。腕を止めると外輪は
    /// −zs / zr で逆に回る。どの役を後ろにしても同じ式を満たす。
    #[test]
    fn planetary_terms_satisfy_the_willis_equation() {
        let c = terms(Member::Carrier, [Member::Sun, Member::Ring], T).unwrap();
        let carrier = eval(c, |m| if m == Member::Sun { 4.0 } else { 0.0 });
        assert!((carrier - 1.0).abs() < 1e-12, "{carrier}");
        let r = terms(Member::Ring, [Member::Sun, Member::Carrier], T).unwrap();
        let ring = eval(r, |m| if m == Member::Sun { 3.0 } else { 0.0 });
        assert!((ring + 1.0).abs() < 1e-12, "{ring}");
        // 太陽を後ろに: どんな腕・外輪の値でも zs θs + zr θr = (zs + zr) θc
        let s = terms(Member::Sun, [Member::Ring, Member::Carrier], T).unwrap();
        let (th_r, th_c) = (0.7, -1.3);
        let th_s = eval(s, |m| if m == Member::Ring { th_r } else { th_c });
        assert!((24.0 * th_s + 72.0 * th_r - 96.0 * th_c).abs() < 1e-9);
        // 係数は歯数の整数で書く(欄の式)。分母は正
        assert!(c.iter().all(|t| t.lead > 0));
        assert_eq!((c[0].lead, c[0].follow), (96, 24));
    }

    /// 遊星の腕に対する自転: 腕を止めて太陽を 1 回すと −zs / zp。太陽からでも外輪からでも同じ値。
    /// 差動(横の歯車の歯数が同じ)は、箱 = 横の 2 つの平均。
    #[test]
    fn planet_spin_and_differential() {
        let p = terms(Member::Planet, [Member::Carrier, Member::Sun], T).unwrap();
        let spin = eval(p, |m| if m == Member::Sun { 1.0 } else { 0.0 });
        assert!((spin + 1.0).abs() < 1e-12, "{spin}");
        // 太陽 2・腕 0.5 なら外輪は (96 × 0.5 − 24 × 2) / 72 = 0。外輪からの自転も同じ
        let (s, c) = (2.0, 0.5);
        let r = (96.0 * c - 24.0 * s) / 72.0;
        let by_sun = eval(p, |m| if m == Member::Sun { s } else { c });
        let q = terms(Member::Planet, [Member::Ring, Member::Carrier], T).unwrap();
        let by_ring = eval(q, |m| if m == Member::Ring { r } else { c });
        assert!((by_sun - by_ring).abs() < 1e-12, "{by_sun} {by_ring}");
        // 差動: 横 1 が 1、横 2 が 3 なら箱は 2
        let d = Teeth {
            sun: 16,
            ring: 16,
            planet: Some(10),
        };
        let case = terms(Member::Carrier, [Member::Sun, Member::Ring], d).unwrap();
        let v = eval(case, |m| if m == Member::Sun { 1.0 } else { 3.0 });
        assert!((v - 2.0).abs() < 1e-12, "{v}");
        // 合わない組・歯数は誤り
        assert!(terms(Member::Planet, [Member::Sun, Member::Ring], T).is_err());
        assert!(terms(Member::Ring, [Member::Ring, Member::Sun], T).is_err());
        let bad = Teeth { sun: 0, ..T };
        assert!(terms(Member::Ring, [Member::Sun, Member::Carrier], bad).is_err());
    }
}
