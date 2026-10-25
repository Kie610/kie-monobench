//! モーション スタディ。
//!
//! いくつものジョイントの値を、0〜[`STUDY_STEPS`] のステップの帯の上のキーで決め、一緒に動かして
//! 機構の動きを確かめる。キーの間は直線で結び、最初のキーの前は最初のキーの値、最後のキーの後は
//! 最後のキーの値のまま。ストーリーボード(アニメーションの段)と違い、時間を並べず、ジョイントごとの
//! 値の線を重ねる。
//!
//! 設計の形・値・部品表は変えない。3D のビューだけが [`body_poses`] の行列でボディを動かして描く
//! (アニメーションと同じ [`crate::animation::joint_poses`])。
//!
//! 文書には組立のモジュール(jc-asm)が名乗る区画 [`MotionStudies`](`asm.motion`)として入る
//! (版 3 までの文書では上の階の `motion_studies`。読む時に区画へ移す)。

use jc_core::kernel::Matrix4;
use jc_core::{DocumentData, FeatureId, RecomputeReport, Registry, SectionType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// ステップの帯の長さ。
pub const STUDY_STEPS: u32 = 100;
/// 端から端まで再生する長さの既定(秒)。
pub const STUDY_SECONDS: f64 = 4.0;
/// 1 つの文書に持てるスタディの数の上限。
const MAX_STUDIES: usize = 50;

/// ジョイントの値を決めるキー 1 つ。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MotionKey {
    /// ステップ(0〜[`STUDY_STEPS`])
    pub step: u32,
    /// 値(回転は度、スライダは mm。ジョイントの欄と同じ見せる単位)
    pub value: f64,
}

/// 1 つのジョイントのキーの並び(ステップの順、同じステップは 1 つ)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionTrack {
    pub joint: FeatureId,
    #[serde(default)]
    pub keys: Vec<MotionKey>,
}

impl MotionTrack {
    /// ステップ `s`(途中の値でもよい)の値。キーが無ければ `None`。
    pub fn value_at(&self, s: f64) -> Option<f64> {
        let first = self.keys.first()?;
        if s <= first.step as f64 {
            return Some(first.value);
        }
        for w in self.keys.windows(2) {
            let (a, b) = (w[0], w[1]);
            if s <= b.step as f64 {
                let f = (s - a.step as f64) / (b.step - a.step) as f64;
                return Some(a.value + (b.value - a.value) * f);
            }
        }
        self.keys.last().map(|k| k.value)
    }

    /// ステップ `step` のキーを `value` にする(無ければ足す)。並びの順は保つ。
    pub fn set_key(&mut self, step: u32, value: f64) {
        match self.keys.binary_search_by_key(&step, |k| k.step) {
            Ok(i) => self.keys[i].value = value,
            Err(i) => self.keys.insert(i, MotionKey { step, value }),
        }
    }

    /// ステップ `step` のキーを消す。あったら真。
    pub fn remove_key(&mut self, step: u32) -> bool {
        match self.keys.binary_search_by_key(&step, |k| k.step) {
            Ok(i) => {
                self.keys.remove(i);
                true
            }
            Err(_) => false,
        }
    }
}

/// モーション スタディ 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionStudy {
    pub name: String,
    #[serde(default)]
    pub tracks: Vec<MotionTrack>,
    /// 0 から [`STUDY_STEPS`] まで再生する長さ(秒)
    #[serde(default = "default_seconds")]
    pub seconds: f64,
}

fn default_seconds() -> f64 {
    STUDY_SECONDS
}

impl MotionStudy {
    /// ジョイントの無い、名前 `name` のスタディ。
    pub fn new(name: impl Into<String>) -> MotionStudy {
        MotionStudy {
            name: name.into(),
            tracks: Vec::new(),
            seconds: STUDY_SECONDS,
        }
    }

    /// ジョイント `joint` の並び(無ければ空の並びを足す)。
    pub fn track_mut(&mut self, joint: FeatureId) -> &mut MotionTrack {
        let i = match self.tracks.iter().position(|t| t.joint == joint) {
            Some(i) => i,
            None => {
                self.tracks.push(MotionTrack {
                    joint,
                    keys: Vec::new(),
                });
                self.tracks.len() - 1
            }
        };
        &mut self.tracks[i]
    }

    /// ステップ `s` の、ジョイントごとの値(見せる単位)。キーの無いジョイントは入らない。
    pub fn values_at(&self, s: f64) -> BTreeMap<FeatureId, f64> {
        self.tracks
            .iter()
            .filter_map(|t| Some((t.joint, t.value_at(s)?)))
            .collect()
    }
}

/// 文書の区画 `asm.motion`: モーション スタディの並び。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MotionStudies {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studies: Vec<MotionStudy>,
    /// このアプリの知らない項目(新しいアプリが足した物)。読んだまま書き戻す
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl SectionType for MotionStudies {
    const KEY: &'static str = "asm.motion";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "モーション スタディ";

    fn check(&self) -> Result<(), String> {
        check_studies(&self.studies)
    }

    fn is_empty(&self) -> bool {
        self.studies.is_empty() && self.rest.is_empty()
    }
}

/// 文書のモーション スタディ(画面の一覧に使う)。区画を読めなければログに残して空。
pub fn studies(data: &DocumentData) -> Vec<MotionStudy> {
    data.section::<MotionStudies>()
        .map(|m| m.studies)
        .unwrap_or_else(|e| {
            log::warn!("{e}");
            Vec::new()
        })
}

/// まだ使われていない「モーション スタディN」の名前。
pub fn free_name(list: &[MotionStudy]) -> String {
    (1..)
        .map(|i| format!("モーション スタディ{i}"))
        .find(|n| list.iter().all(|s| &s.name != n))
        .expect("どこかで空く")
}

/// スタディの並びが使える値か(名前が空でなく重ならない、ジョイントが重ならない、キーはステップの
/// 順で範囲の中、値と長さが有限)。ジョイントが今あるかは問わない(取り消しで戻るため)。
pub fn check_studies(list: &[MotionStudy]) -> Result<(), String> {
    if list.len() > MAX_STUDIES {
        return Err(format!("スタディは {MAX_STUDIES} 個までです"));
    }
    for (i, s) in list.iter().enumerate() {
        if s.name.trim().is_empty() {
            return Err("スタディの名前が空です".into());
        }
        if list[..i].iter().any(|o| o.name == s.name) {
            return Err(format!("スタディの名前「{}」が重なっています", s.name));
        }
        if !(s.seconds.is_finite() && s.seconds > 0.0 && s.seconds <= 600.0) {
            return Err(format!(
                "「{}」の長さは 0 より長く 600 秒までにしてください(今 {})",
                s.name, s.seconds
            ));
        }
        for (j, t) in s.tracks.iter().enumerate() {
            if s.tracks[..j].iter().any(|o| o.joint == t.joint) {
                return Err(format!(
                    "「{}」にジョイント {} が 2 回あります",
                    s.name, t.joint
                ));
            }
            for (n, k) in t.keys.iter().enumerate() {
                if k.step > STUDY_STEPS {
                    return Err(format!(
                        "キーのステップは 0〜{STUDY_STEPS} です(今 {})",
                        k.step
                    ));
                }
                if !k.value.is_finite() {
                    return Err(format!("ステップ {} の値が数ではありません", k.step));
                }
                if n > 0 && t.keys[n - 1].step >= k.step {
                    return Err("キーはステップの順に、同じステップは 1 つにしてください".into());
                }
            }
        }
    }
    Ok(())
}

/// スタディ `study` をステップ `s` にした時の、表示しているボディ(最後の機能)ごとの動き。
/// 動かないボディは入らない。
pub fn body_poses(
    data: &DocumentData,
    registry: &Registry,
    report: &RecomputeReport,
    study: &MotionStudy,
    s: f64,
) -> BTreeMap<FeatureId, Matrix4> {
    let by_root = crate::animation::joint_poses(data, registry, report, &study.values_at(s));
    crate::animation::bodies_of_roots(data, registry, report, &by_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(keys: &[(u32, f64)]) -> MotionTrack {
        MotionTrack {
            joint: FeatureId(1),
            keys: keys
                .iter()
                .map(|&(step, value)| MotionKey { step, value })
                .collect(),
        }
    }

    #[test]
    fn values_follow_straight_lines_between_the_keys() {
        let t = track(&[(20, 0.0), (60, 90.0), (100, 30.0)]);
        // 最初のキーの前は最初の値、最後の後は最後の値
        assert_eq!(t.value_at(0.0), Some(0.0));
        assert_eq!(t.value_at(20.0), Some(0.0));
        assert!((t.value_at(40.0).unwrap() - 45.0).abs() < 1e-9);
        assert!((t.value_at(80.0).unwrap() - 60.0).abs() < 1e-9);
        assert_eq!(t.value_at(100.0), Some(30.0));
        assert_eq!(t.value_at(120.0), Some(30.0));
        assert_eq!(track(&[]).value_at(10.0), None);
        // キー 1 つなら、どこでもその値
        assert_eq!(track(&[(50, 12.0)]).value_at(0.0), Some(12.0));
    }

    #[test]
    fn keys_stay_in_order_and_one_per_step() {
        let mut t = track(&[]);
        t.set_key(50, 1.0);
        t.set_key(10, 2.0);
        t.set_key(50, 3.0);
        assert_eq!(
            t.keys.iter().map(|k| (k.step, k.value)).collect::<Vec<_>>(),
            vec![(10, 2.0), (50, 3.0)]
        );
        assert!(t.remove_key(10));
        assert!(!t.remove_key(10));
        let mut s = MotionStudy::new("スタディ");
        s.tracks.push(t);
        assert!(check_studies(std::slice::from_ref(&s)).is_ok());
    }

    #[test]
    fn bad_studies_are_refused() {
        let ok = MotionStudy::new("a");
        assert!(
            check_studies(&[ok.clone(), ok.clone()]).is_err(),
            "名前の重なり"
        );
        assert!(check_studies(&[MotionStudy::new(" ")]).is_err(), "空の名前");
        let mut s = ok.clone();
        s.seconds = 0.0;
        assert!(check_studies(&[s]).is_err(), "長さ 0");
        let mut s = ok.clone();
        s.tracks.push(track(&[(10, 0.0), (10, 1.0)]));
        assert!(check_studies(&[s]).is_err(), "同じステップ");
        let mut s = ok.clone();
        s.tracks.push(track(&[(101, 0.0)]));
        assert!(check_studies(&[s]).is_err(), "範囲の外");
        let mut s = ok.clone();
        s.tracks.push(track(&[(1, f64::NAN)]));
        assert!(check_studies(&[s]).is_err(), "数でない値");
        let mut s = ok;
        s.tracks.push(track(&[]));
        s.tracks.push(track(&[]));
        assert!(check_studies(&[s]).is_err(), "同じジョイント");
    }
}
