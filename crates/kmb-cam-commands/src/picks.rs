//! 加工が参照する底の面を、3D のビューの選びで後から変える操作。
//!
//! 底の面を持つ加工(戦略の名乗りの [`Strategy::floor_faces`]。ポケットと負荷制御粗取り)は、
//! 底の面を足す・外す(Shift・Ctrl+クリックで面を選んでから押す)。機能のデータ(`face`・`faces`)
//! だけを書き換える。面取りの辺の選び直しは、面取りの戦略の crate が持つ。

use kmb_cam_setup::Strategy;
use kmb_core::{Action, Context, Edit, FeatureId, Outcome, Place};
use serde_json::{Value, json};

pub const ADD_FACES: &str = "cam.floor.add";
pub const REMOVE_FACES: &str = "cam.floor.remove";

/// 選んでいる機能が底の面を持つ加工なら、その ID とデータ。
fn selected_floor(cx: &Context<'_>, all: &[Strategy]) -> Option<(FeatureId, Value)> {
    let id = cx.selected?;
    let f = cx.data.feature(id)?;
    all.iter()
        .any(|s| s.floor_faces && s.type_id == f.type_id)
        .then(|| (id, f.data.clone()))
}

/// データの底の面の全部(`face` が先)。
fn floor_faces(d: &Value) -> Vec<String> {
    let face = d
        .get("face")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let more = d
        .get("faces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    face.into_iter().chain(more).map(String::from).collect()
}

/// 底の面の全部を、データの `face` と `faces` に入れる。
fn set_floor_faces(d: &mut Value, faces: &[String]) {
    let Some(o) = d.as_object_mut() else {
        return;
    };
    match faces.first() {
        Some(f) => o.insert("face".into(), json!(f)),
        None => o.remove("face"),
    };
    if faces.len() > 1 {
        o.insert("faces".into(), json!(faces[1..]));
    } else {
        o.remove("faces");
    }
}

/// 3D のビューで選んでいる平らな面の名前(選んだ順)。
fn picked_planar(cx: &Context<'_>) -> Vec<String> {
    cx.picked_faces()
        .into_iter()
        .filter(|f| {
            Context {
                picked: Some(f),
                ..*cx
            }
            .picked_plane()
            .is_some()
        })
        .map(|f| f.face.clone())
        .collect()
}

pub fn actions(cx: &Context<'_>, all: &[Strategy]) -> Vec<Action> {
    let mut v = Vec::new();
    if let Some((_, d)) = selected_floor(cx, all) {
        let have = floor_faces(&d);
        let picked = picked_planar(cx);
        let new = picked.iter().filter(|f| !have.contains(f)).count();
        let gone = picked.iter().filter(|f| have.contains(f)).count();
        let sketch = d.get("profile").is_some_and(|p| !p.is_null());
        v.push(
            Action::new(ADD_FACES, "選んだ面を足す", "cam")
                .menu("2D")
                .icon("pocket")
                .aliases("めんをたす そこ ふくすう add face floor")
                .hint(
                    "3D のビューで選んだ平らな面を、底の面に足す(Shift・Ctrl+クリックで複数選べる)",
                )
                .at(&[Place::Panel, Place::FeatureMenu])
                .disabled_if(
                    (new == 0)
                        .then_some("3D のビューで、まだ入っていない平らな底の面を選んでから使う"),
                ),
        );
        v.push(
            Action::new(REMOVE_FACES, "選んだ面を外す", "cam")
                .menu("2D")
                .icon("pocket")
                .aliases("めんをはずす そこ remove face floor")
                .hint("3D のビューで選んだ面を、底の面から外す")
                .at(&[Place::Panel, Place::FeatureMenu])
                .disabled_if(if gone == 0 {
                    Some("3D のビューで、底の面に入っている面を選んでから使う")
                } else if gone >= have.len() && !sketch {
                    Some("底の面を全部は外せません(ほかの面を足してから外す)")
                } else {
                    None
                }),
        );
    }
    v
}

pub fn run(id: &str, cx: &Context<'_>, all: &[Strategy]) -> Option<Result<Outcome, String>> {
    if ![ADD_FACES, REMOVE_FACES].contains(&id) {
        return None;
    }
    Some((|| {
        let (fid, mut d) = selected_floor(cx, all).ok_or("底の面を持つ加工を選んでいません")?;
        let mut faces = floor_faces(&d);
        let picked = picked_planar(cx);
        let label = if id == ADD_FACES {
            let new: Vec<String> = picked.into_iter().filter(|f| !faces.contains(f)).collect();
            if new.is_empty() {
                return Err("足す平らな面を選んでいません".into());
            }
            faces.extend(new);
            "底の面を足す"
        } else {
            let before = faces.len();
            faces.retain(|f| !picked.contains(f));
            if faces.len() == before {
                return Err("外す面を選んでいません".into());
            }
            let sketch = d.get("profile").is_some_and(|p| !p.is_null());
            if faces.is_empty() && !sketch {
                return Err("底の面を全部は外せません".into());
            }
            "底の面を外す"
        };
        set_floor_faces(&mut d, &faces);
        Ok(Outcome::Change {
            label: label.into(),
            edits: vec![Edit::SetData { id: fid, data: d }],
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_faces_round_trip_through_face_and_faces() {
        let mut d = json!({"face": "f1:end", "leave": "0 mm"});
        assert_eq!(floor_faces(&d), vec!["f1:end"]);
        set_floor_faces(
            &mut d,
            &["f1:end".into(), "f2:start".into(), "f3:start".into()],
        );
        assert_eq!(d["faces"], json!(["f2:start", "f3:start"]));
        assert_eq!(floor_faces(&d).len(), 3);
        // 先頭を外すと、次の面が `face` になる
        set_floor_faces(&mut d, &["f2:start".into()]);
        assert_eq!(d["face"], json!("f2:start"));
        assert!(d.get("faces").is_none());
        set_floor_faces(&mut d, &[]);
        assert!(d.get("face").is_none());
        assert_eq!(d["leave"], json!("0 mm"));
    }
}
