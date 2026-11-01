//! 画面を CPU で絵にして書き出す(GPU のない環境で、実際のアプリの見た目を確かめるため)。
//!
//! egui が出す三角形(文字の絵と 3D のビューの絵を含む)をそのまま塗るので、画面と同じものが出る。
//!
//! `JC_SCREENS_OUT=dir cargo test -p kmb-app --test screens -- --ignored`
//! (`dir` に場面ごとの BMP を書く。`JC_SCREENS_NEW=1` なら、新しい工具の種類も足す。
//! 日本語のフォントがアプリの候補にない Linux では `JC_FONT` でフォントのパスを渡す)
//!
//! 画面の大きさと倍率は `JC_SCREENS_SIZE=1920x1034`(論理の大きさ)と `JC_SCREENS_SCALE=2`
//! (Windows の表示倍率 200%)で変えられる。既定は 1920×1080、倍率 1。
//!
//! 人の操作の e2e(`ui::<台本の名前>`、`--ignored` なしで走る): `tests/ui/*.jcs` のコマンド欄の
//! スクリプトを、台本ごとに 1 つのテストとして人の手順どおりに画面を通して流す(部品を名前で探し、
//! マウスとキーで押す・打つ。中身は `kmb_ui::drive`・コマンド欄)。1 本だけ流すなら
//! `cargo test -p kmb-app --test screens ui::熱の単位`。`shot <名前>` の写真は
//! `JC_SCREENS_OUT` がある時だけ書く(`JC_SCREENS_BRIGHTNESS=light` なら明るい画面で撮る)。先頭の `#! sample part|cam|cam3d|box` で始める文書を選ぶ。
//! `#! size 1280x800` で画面の論理の大きさを決める(`JC_SCREENS_SIZE` があればそちら)。
//! `#! fonts` で kmb-text の試験のフォントを文字の編集器に足す。
//! `#! open <パス>` は、開く窓で人がそのファイル(apps/kmb-app からのパス)を選んだことにする。

use egui::epaint::{ClippedPrimitive, ImageDelta, Primitive};
use egui::{Color32, ColorImage, ImageData, TextureId};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::reference_cam_document;
use kmb_ui::{Space, Workspace};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 画面の論理の大きさと倍率(`JC_SCREENS_SIZE`・`JC_SCREENS_SCALE`)。
fn size_and_scale() -> ([f32; 2], f32) {
    let size = std::env::var("JC_SCREENS_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.parse().ok()?, h.parse().ok()?])
        })
        .unwrap_or([1920.0, 1080.0]);
    let scale = std::env::var("JC_SCREENS_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    (size, scale)
}

/// 画面の絵。色は乗算済みの sRGB。
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas {
            w,
            h,
            px: vec![[0.0, 0.0, 0.0, 1.0]; w * h],
        }
    }

    fn image(&self) -> ColorImage {
        let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        ColorImage::new(
            [self.w, self.h],
            self.px
                .iter()
                .map(|p| Color32::from_rgb(to(p[0]), to(p[1]), to(p[2])))
                .collect(),
        )
    }
}

/// 貼る絵(egui の手で作られた文字の絵と、利用者の絵)。
#[derive(Default)]
struct Textures(HashMap<TextureId, ColorImage>);

impl Textures {
    fn apply<'a>(
        &mut self,
        set: impl IntoIterator<Item = (&'a TextureId, &'a ImageDelta)>,
        free: impl IntoIterator<Item = &'a TextureId>,
    ) {
        for (id, d) in set {
            let ImageData::Color(img) = &d.image;
            match d.pos {
                None => {
                    self.0.insert(*id, (**img).clone());
                }
                Some([x0, y0]) => {
                    let t = self.0.get_mut(id).expect("部分の更新の前に全体がある");
                    for y in 0..img.size[1] {
                        for x in 0..img.size[0] {
                            t.pixels[(y0 + y) * t.size[0] + x0 + x] =
                                img.pixels[y * img.size[0] + x];
                        }
                    }
                }
            }
        }
        for id in free {
            self.0.remove(id);
        }
    }

    /// 双線形で拾う(乗算済み、0〜1)。
    fn sample(&self, id: TextureId, u: f32, v: f32) -> [f32; 4] {
        let Some(t) = self.0.get(&id) else {
            return [1.0; 4];
        };
        let [w, h] = t.size;
        let x = (u * w as f32 - 0.5).clamp(0.0, w as f32 - 1.0);
        let y = (v * h as f32 - 0.5).clamp(0.0, h as f32 - 1.0);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p = |x: usize, y: usize| t.pixels[y * w + x].to_array().map(|c| c as f32 / 255.0);
        let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
        std::array::from_fn(|i| {
            (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy
        })
    }
}

fn paint(canvas: &mut Canvas, prims: &[ClippedPrimitive], tex: &Textures, ppp: f32) {
    for cp in prims {
        let Primitive::Mesh(m) = &cp.primitive else {
            continue;
        };
        let clip = cp.clip_rect;
        let cx0 = ((clip.min.x * ppp).max(0.0) as usize).min(canvas.w);
        let cy0 = ((clip.min.y * ppp).max(0.0) as usize).min(canvas.h);
        let cx1 = ((clip.max.x * ppp).ceil().max(0.0) as usize).min(canvas.w);
        let cy1 = ((clip.max.y * ppp).ceil().max(0.0) as usize).min(canvas.h);
        for t in m.indices.chunks(3) {
            let v = [
                m.vertices[t[0] as usize],
                m.vertices[t[1] as usize],
                m.vertices[t[2] as usize],
            ];
            let p = v.map(|v| [v.pos.x * ppp, v.pos.y * ppp]);
            let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
            if area.abs() < 1e-9 {
                continue;
            }
            let minx = p.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min);
            let maxx = p.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max);
            let miny = p.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min);
            let maxy = p.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max);
            let x0 = (minx.floor().max(0.0) as usize).max(cx0);
            let x1 = (maxx.ceil().max(0.0) as usize).min(cx1);
            let y0 = (miny.floor().max(0.0) as usize).max(cy0);
            let y1 = (maxy.ceil().max(0.0) as usize).min(cy1);
            let col = v.map(|v| v.color.to_array().map(|c| c as f32 / 255.0));
            for y in y0..y1 {
                for x in x0..x1 {
                    let (sx, sy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let e = |a: [f32; 2], b: [f32; 2]| {
                        (b[0] - a[0]) * (sy - a[1]) - (sx - a[0]) * (b[1] - a[1])
                    };
                    let w0 = e(p[1], p[2]) / area;
                    let w1 = e(p[2], p[0]) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                    let vv = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                    let s = tex.sample(m.texture_id, u, vv);
                    let src: [f32; 4] = std::array::from_fn(|i| {
                        (w0 * col[0][i] + w1 * col[1][i] + w2 * col[2][i]) * s[i]
                    });
                    let d = &mut canvas.px[y * canvas.w + x];
                    for i in 0..4 {
                        d[i] = src[i] + d[i] * (1.0 - src[3]);
                    }
                }
            }
        }
    }
}

/// 画面の試験の描き手の代わりに、送られた絵をためておく。
struct Capture(Arc<Mutex<Textures>>);

impl egui_kittest::TestRenderer for Capture {
    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        let mut t = self.0.lock().unwrap();
        t.apply(
            delta
                .set
                .iter()
                .flat_map(|(id, ds)| ds.iter().map(move |d| (id, d))),
            [],
        );
        t.apply([], &delta.free);
        delta.clear();
    }
}

/// 画面を動かしながら、貼る絵を集めておく。
struct Screen {
    h: Harness<'static, Workspace>,
    tex: Arc<Mutex<Textures>>,
    /// 画面の論理の大きさ
    size: [f32; 2],
}

impl Screen {
    fn new(ws: Workspace) -> Screen {
        Screen::sized(ws, None)
    }

    /// `size` の論理の大きさで作る(`None` なら `JC_SCREENS_SIZE`)。
    fn sized(ws: Workspace, size: Option<[f32; 2]>) -> Screen {
        let tex = Arc::new(Mutex::new(Textures::default()));
        let (env_size, scale) = size_and_scale();
        let size = size.unwrap_or(env_size);
        // 最初は小さく作り、3D の絵の上限を上げてから広げる(倍率 2 の 3D の絵は 2048 より大きい)
        let mut h = Harness::builder()
            .with_size(egui::vec2(320.0, 240.0))
            .with_pixels_per_point(scale)
            .renderer(Capture(tex.clone()))
            .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
        // アプリと同じく OS の日本語のフォントを読む(Linux なら JC_FONT で渡す)。
        // 控えの字体もこの場で読む(アプリは裏で読むので、撮る時に有ったり無かったりしないように)
        kmb_ui::install_fonts(&h.ctx, None);
        h.input_mut().max_texture_side = Some(8192);
        h.set_size(egui::vec2(size[0], size[1]));
        let mut s = Screen { h, tex, size };
        s.settle();
        // 小さく作った時の視点の合わせ方が残るので、広げた大きさで全体を見直す
        // (3D のビューの無い画面(ホームなど)にはボタンが無いので、押せなくても進む)
        let _ = s.click_exact("全体表示");
        s
    }

    /// 落ち着くまで回す(1 こまごとに絵の更新を拾う)。
    fn settle(&mut self) {
        for _ in 0..12 {
            self.h.step();
        }
    }

    /// `label` を含む部品を押す(2 つ以上あれば panic)。見つからなければ何もせず `false`。
    /// 押せないまま次の写真を撮らないよう、呼ぶ側は `assert!` か `if` で受ける。
    #[must_use]
    fn click(&mut self, label: &str) -> bool {
        match self.h.query_by_label_contains(label) {
            Some(n) => {
                n.click();
                self.settle();
                true
            }
            None => false,
        }
    }

    /// 名前がちょうど `label` の部品を押す(コマンド欄・MCP の `click` と同じ探し方)。
    /// 見つからなければ何もせず `false`。▾ の付くボタンの名前は ▾ を除いた字(`theme::icon_text_button_fit`)。
    #[must_use]
    fn click_exact(&mut self, label: &str) -> bool {
        match self.h.query_by_label(label) {
            Some(n) => {
                n.click();
                self.settle();
                true
            }
            None => false,
        }
    }

    fn write(&mut self, dir: &str, name: &str) {
        self.h.step();
        let o = self.h.output().clone();
        let ppp = o.pixels_per_point;
        let prims = self.h.ctx.tessellate(o.shapes, ppp);
        let mut tex = self.tex.lock().unwrap();
        // 文字の絵は今の全体をもらう(後から足したフォントの字も入る)
        tex.0
            .insert(TextureId::default(), self.h.ctx.fonts(|f| f.image()));
        let mut c = Canvas::new(
            (self.size[0] * ppp).round() as usize,
            (self.size[1] * ppp).round() as usize,
        );
        paint(&mut c, &prims, &tex, ppp);
        write_bmp(&format!("{dir}/{name}.bmp"), &c.image());
    }
}

fn first_of(ws: &Workspace, type_id: &str) -> kmb_core::FeatureId {
    ws.document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == type_id)
        .unwrap()
        .id
}

/// 基準部品の加工の文書。`new` なら工具にテーパー(先 0.5 mm、突き出しとホルダーの径つき)を足す。
fn document(new: bool) -> kmb_core::Document {
    let mut doc = reference_cam_document();
    if new {
        let tools = doc
            .data()
            .features
            .iter()
            .find(|f| f.type_id == "cam.tools@2")
            .unwrap();
        let (id, mut data) = (tools.id, tools.data.clone());
        let mut t = data["tools"][1].clone();
        t["number"] = 8.into();
        t["name"] = "テーパー 0.5 mm".into();
        t["kind"] = "taper_end".into();
        t["tip_angle"] = 10.0.into();
        t["tip_diameter"] = 0.5.into();
        t["flute_length"] = 12.0.into();
        t["stickout"] = 20.0.into();
        t["holder_diameter"] = 16.0.into();
        data["tools"].as_array_mut().unwrap().push(t);
        doc.apply(kmb_core::Transaction::new(
            "工具を足す",
            vec![kmb_core::Op::SetFeatureData { id, data }],
        ))
        .unwrap();
    }
    doc
}

/// スクリプトの始めの文書(`#! sample part|cam|cam3d|box|laser|sketch|sweep|sketch3d-line|empty`。既定は part。
/// box は蓋つきの箱、sketch3d-line は part に Z に立てた線 1 本の 3D スケッチを足したもの、
/// laser は最初のスケッチの中を塗るレーザーの加工を足した加工の見本、sketch は作図の道具の見本
/// (一部の位置が決まっていないスケッチ)、sweep はスイープとロフトの断面と経路のスケッチ、empty は空の文書)。
fn script_document(text: &str) -> kmb_core::Document {
    let kind = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("#!"))
        .filter_map(|l| l.trim().strip_prefix("sample"))
        .map(|k| k.trim().to_string())
        .next()
        .unwrap_or_else(|| "part".into());
    match kind.as_str() {
        "cam" => reference_cam_document(),
        "cam3d" => kmb_samples::reference_cam_3d_document(),
        "box" => lidded_box_document(),
        "box-knob" => lidded_box_with_knob_joint(),
        "laser" => laser_document(),
        "sketch" => kmb_samples::sketch_tools_document(),
        "sweep" => kmb_samples::sweep_loft_document(),
        "sketch3d-line" => sketch3d_line_document(),
        "empty" => kmb_core::Document::new(),
        _ => kmb_samples::reference_document(),
    }
}

/// スクリプトの先頭の `#! size 1280x800` の画面の論理の大きさ。`JC_SCREENS_SIZE` があればそちら
/// (PC と同じ 4K・倍率 200 % で流す時)。
fn script_size(text: &str) -> Option<[f32; 2]> {
    if std::env::var("JC_SCREENS_SIZE").is_ok() {
        return None;
    }
    let s = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("#!"))
        .find_map(|l| l.trim().strip_prefix("size"))?;
    let (w, h) = s.trim().split_once('x')?;
    Some([w.parse().ok()?, h.parse().ok()?])
}

/// 台本の保存の窓(先頭に `#! files` のある台本だけ): 人が「保存」を押したのと同じく、勧めた名前で
/// `dir` の下に書く(文書の保存も同じ所。版の履歴とレンダリング ギャラリーはその横の .jc に残る)。何度も書き出す台本のために、名前の頭に書き出した順の番号を付ける(1-無題.3mf)。
struct ScriptDialogs {
    dir: std::path::PathBuf,
    count: Mutex<usize>,
    /// 開く窓で選ぶファイル(`#! open <apps/kmb-app からのパス>`)
    open: Option<std::path::PathBuf>,
}

impl kmb_ui::Dialogs for ScriptDialogs {
    fn open_document(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn save_document(&self, name: &str) -> Option<std::path::PathBuf> {
        Some(self.dir.join(name))
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
        self.open.clone()
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], name: &str) -> Option<std::path::PathBuf> {
        let mut n = self.count.lock().unwrap();
        *n += 1;
        Some(self.dir.join(format!("{n}-{name}")))
    }
    fn unsaved(&self, _: &str) -> kmb_ui::Unsaved {
        kmb_ui::Unsaved::Cancel
    }
}

/// 台本の開く窓で人が選ぶファイル(先頭に `#! open <apps/kmb-app からのパス>` のある台本だけ)。
fn script_open(text: &str) -> Option<std::path::PathBuf> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("#!"))
        .find_map(|l| l.trim().strip_prefix("open "))
        .map(|p| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(p.trim()))
}

/// 台本のプロジェクトの場(`#! project` の台本だけ)。一時フォルダに「棚」(天板・台座、下に「部品」
/// フォルダと脚)を作ってプロジェクトに登録し、そのデータフォルダを返す(前の回の物は消す)。
fn script_project(name: &str, text: &str) -> Option<std::path::PathBuf> {
    if !text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("#!"))
        .any(|l| l.trim() == "project")
    {
        return None;
    }
    let dir = std::env::temp_dir().join("kmb-ui-projects").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let shelf = dir.join("棚");
    for (f, sample) in [
        (shelf.join("天板.json"), kmb_samples::reference_document()),
        (shelf.join("台座.json"), kmb_samples::reference_document()),
        (
            shelf.join("部品").join("脚.json"),
            kmb_samples::sample_document(),
        ),
    ] {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        kmb_session::save(&f, &sample).unwrap();
    }
    let data = dir.join("data");
    let mut p = kmb_session::projects::Projects::default();
    p.add(&shelf).unwrap();
    p.save(&data).unwrap();
    Some(data)
}

/// 台本が書き出したファイルを置く所(`#! files` の台本だけ。前の回の物は消す)。
fn script_files(name: &str, text: &str) -> Option<std::path::PathBuf> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("#!"))
        .any(|l| l.trim() == "files")
        .then(|| {
            let dir = std::env::temp_dir().join("kmb-ui-scripts").join(name);
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        })
}

/// 台本が書き出したファイルの中身を確かめる(画面の操作だけでは見えない所。台本の名前ごと)。
fn check_script_files(name: &str, dir: &std::path::Path) -> Result<(), String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();
    let names: Vec<String> = files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    let read = |ext: &str, k: usize| -> Result<Vec<u8>, String> {
        let f = files
            .iter()
            .filter(|f| f.extension().is_some_and(|x| x == ext))
            .nth(k)
            .ok_or_else(|| format!(".{ext} の {} 本目がありません: {names:?}", k + 1))?;
        std::fs::read(f).map_err(|e| e.to_string())
    };
    let need = |ok: bool, what: &str| if ok { Ok(()) } else { Err(what.to_string()) };
    match name {
        "3MFとSVGの書き出し" => {
            // 3MF: 設定で替えた文書の単位(in)と、ボディの色(既定の鋼)が入る
            let z = read("3mf", 0)?;
            let model = kmb_io_stl::threemf::unzip_part(&z, "3D/3dmodel.model")
                .ok_or("3MF に 3D/3dmodel.model がありません")?;
            let model = String::from_utf8_lossy(&model);
            need(model.contains("unit=\"inch\""), "3MF の単位が inch でない")?;
            need(
                model.contains("<base name=\"鋼\" displaycolor=\"#"),
                "3MF に鋼の色が無い",
            )?;
            need(
                model.contains("<object id=\"2\" type=\"model\""),
                "3MF にボディが無い",
            )?;
            // 今のビューの SVG とスケッチの SVG: 大きさは mm、線は黒で塗らない
            for k in 0..2 {
                let svg = String::from_utf8(read("svg", k)?).map_err(|e| e.to_string())?;
                need(svg.contains("mm\" height=\""), "SVG の大きさが mm でない")?;
                need(
                    svg.contains("fill=\"none\" stroke=\"#000000\""),
                    "SVG の線が黒でない",
                )?;
                need(svg.contains(" Z\""), "SVG に閉じた輪郭が無い")?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// スクリプト 1 本を画面を通して流す。落ちた時は行ごとの記録を返す。
fn run_ui_script(name: &str, text: &str) -> Result<String, String> {
    let files = script_files(name, text);
    // `#! fonts` の台本は、PC のフォントに kmb-text の試験のフォントを足す(どの PC でも同じ字がある)
    let mut text_editor = kmb_ui_text::TextEditor::new();
    if text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("#!"))
        .any(|l| l.trim() == "fonts")
    {
        text_editor = text_editor.with_font_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/kmb-text/tests/fonts"),
        );
    }
    let mut ws = Workspace::new(script_document(text), None, kmb_bundle::modules())
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new())
        .with_editor(text_editor)
        .with_home(kmb_ui_library::Library::new())
        .with_samples(
            kmb_bundle::Sample::ALL
                .iter()
                .map(|s| (s.label().to_string(), s.make()))
                .collect(),
        );
    if let Some(data) = script_project(name, text) {
        ws = ws.with_data_dir(data);
    }
    let open = script_open(text);
    if files.is_some() || open.is_some() {
        ws = ws.with_dialogs(Box::new(ScriptDialogs {
            dir: files.clone().unwrap_or_else(std::env::temp_dir),
            count: Mutex::new(0),
            open,
        }));
    }
    // モジュールの設定(シーンの設定・寸法の見せ方など)はプロセス全体の物で、前に流した台本が
    // 変えた値が残る(縦横比が前の台本の 1:1 のままで、次の台本の高さの欄が消えた)。人が JC を
    // 初めて起動した時と同じく、台本ごとに初めの値から始める
    let reset = ws.reset_module_settings();
    if !reset.is_empty() {
        log::info!("{name}: 前の台本が変えた設定を初めの値に戻した: {reset:?}");
    }
    // 台本の写真を明るい画面で撮る(`JC_SCREENS_BRIGHTNESS=light`。既定は暗い画面)
    if std::env::var("JC_SCREENS_BRIGHTNESS").is_ok_and(|v| v == "light") {
        ws.layout_mut().brightness = kmb_ui::layout::Brightness::Light;
    }
    let mut s = Screen::sized(ws, script_size(text));
    let out = std::env::var("JC_SCREENS_OUT").ok();
    let batch = s.h.state_mut().run_script(text)?;
    let t0 = std::time::Instant::now();
    loop {
        s.h.step();
        if let Some(shot) = s.h.state_mut().take_shot()
            && let Some(dir) = &out
        {
            std::fs::create_dir_all(dir).ok();
            s.write(dir, &format!("{name}-{shot}"));
        }
        if let Some(r) = s.h.state_mut().script_result(batch) {
            if !r.ok {
                return Err(r.report());
            }
            if let Some(dir) = &files {
                check_script_files(name, dir)
                    .map_err(|e| format!("{}\n書き出したファイル: {e}", r.report()))?;
            }
            return Ok(r.report());
        }
        if t0.elapsed().as_secs() > 600 {
            return Err(format!(
                "終わらない:\n{}",
                s.h.state()
                    .command_log()
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if s.h.state().is_busy() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// 人の操作の e2e: `tests/ui/*.jcs` の台本ごとに 1 つのテスト(`ui::<台本の名前>`。build.rs が
/// 台本のファイルから作る)。別のテストなので nextest が並べて流し、落ちた台本の名前がそのまま出る。
mod ui {
    #![allow(non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/ui_scripts.rs"));
}

/// 台本 1 本を流し、落ちたら行ごとの記録を出して落ちる。
fn ui_script(name: &str) {
    let f = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui")
        .join(format!("{name}.jcs"));
    let text =
        std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("{} を読めない: {e}", f.display()));
    let t0 = std::time::Instant::now();
    match run_ui_script(name, &text) {
        Ok(log) => eprintln!("通過 {name}({:.1} 秒)\n{log}", t0.elapsed().as_secs_f32()),
        Err(log) => panic!("落ちた台本 {name}:\n{log}"),
    }
}

/// 確かめること: 設定の部品から入れた操作表示を、暗明とビューの幅・字の倍率で比べる。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に操作の表示の前後の写真を書く"]
fn write_keycast_screens() {
    let dir = std::env::var("JC_SCREENS_OUT").expect("写真の出力先");
    std::fs::create_dir_all(&dir).unwrap();
    for (theme, brightness) in [
        ("dark", kmb_ui::layout::Brightness::Dark),
        ("light", kmb_ui::layout::Brightness::Light),
    ] {
        for (width, text_scale) in [(1280.0, 1.0), (1280.0, 2.5), (1920.0, 1.0), (1920.0, 2.5)] {
            let mut ws = Workspace::new(
                kmb_samples::reference_document(),
                None,
                kmb_bundle::modules(),
            );
            ws.layout_mut().brightness = brightness;
            ws.layout_mut().appearance.text_scale = text_scale;
            let mut screen = Screen::sized(ws, Some([width, 1000.0]));
            let stem = format!(
                "keycast-{theme}-{}-{}",
                width as u32,
                (text_scale * 100.0) as u32
            );
            screen.write(&dir, &format!("{stem}-before"));
            let script = r#"key Ctrl+,
click "マウスとキー"
type ~"設定を探す" "操作の表示"
shot settings-before
click "押したキーとマウスを画面に出す(録画・解説向け)"
expect "コマンドの名前も出す" exists
shot settings-after
key Ctrl+,
key Ctrl+Z
expect "操作の表示: Ctrl+Z 取り消す" exists
"#;
            let batch = screen.h.state_mut().run_script(script).unwrap();
            let start = std::time::Instant::now();
            loop {
                screen.h.step();
                if let Some(shot) = screen.h.state_mut().take_shot() {
                    screen.write(&dir, &format!("{stem}-{shot}"));
                }
                if let Some(result) = screen.h.state_mut().script_result(batch) {
                    assert!(result.ok, "{}", result.report());
                    break;
                }
                assert!(start.elapsed().as_secs() < 60, "設定の部品が揃わない");
            }
            // 画面へ送るポインターの入力で、押して引いている最中のマウスも撮る。
            let pos = egui::pos2(width * 0.6, 550.0);
            screen.h.hover_at(pos);
            screen.h.drag_at(pos);
            screen.h.step();
            screen.h.hover_at(pos + egui::vec2(10.0, 0.0));
            screen.h.step();
            // 偽機器の軸は実時間で戻るので、写真を描く直前に報告を渡してつなぐ。
            let ws = std::mem::replace(
                screen.h.state_mut(),
                Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules()),
            );
            let device = kmb_ui_mouse3d::Mouse3d::fake();
            device.feed(&[2, 0, 0, 0, 0, 0xA2, 0xFE]);
            *screen.h.state_mut() = ws.with_mouse3d(device);
            // 実際のキー入力を最後の描画フレームへ送り、押したままの札を撮る。
            screen
                .h
                .input_mut()
                .events
                .push(egui::Event::ModifiersChanged(egui::Modifiers::CTRL));
            screen.h.input_mut().events.push(egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            });
            // 行が増えた後の領域の大きさが揃ってから、下端を合わせた写真を描く。
            screen.h.step();
            screen.write(&dir, &format!("{stem}-after"));
            assert!(
                screen
                    .h
                    .query_by_label("操作の表示: Ctrl+Z 取り消す")
                    .is_some()
            );
            assert!(screen.h.query_by_label("操作の表示: マウス 左").is_some());
            assert!(screen.h.query_by_label("ドラッグ").is_some());
            assert!(screen.h.query_by_label("ひねる").is_some());
            screen.h.drop_at(pos + egui::vec2(10.0, 0.0));
        }
    }
}

#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let new = std::env::var("JC_SCREENS_NEW").is_ok();
    let ws = |new| {
        let mut ws = Workspace::new(document(new), None, kmb_bundle::modules())
            .with_editor(kmb_ui_sketch::SketchEditor::new())
            .with_editor(kmb_ui_tools::ToolsEditor::new());
        ws.layout_mut().space = Space::Machining;
        ws
    };

    // 5b. 寸法の札の見本(長さ・横・縦・角度・直径・半径・参照寸法)。
    //     `JC_SCREENS_ONLY_DIMS=1` ならこれだけ書く
    let mut w = Workspace::new(
        kmb_samples::sketch_dims_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_editor(kmb_ui_sketch::SketchEditor::new())
    .with_editor(kmb_ui_tools::ToolsEditor::new());
    let sketch = first_of(&w, "cad.sketch@1");
    w.open_edit(sketch);
    let mut s = Screen::new(w);
    s.write(&dir, "5b-スケッチ-寸法の札");
    if std::env::var("JC_SCREENS_ONLY_DIMS").is_ok() {
        return;
    }
    // 5c. 作業空間の一覧を開いた所(字とアイコンが 100 % と 250 %)。
    //     `JC_SCREENS_ONLY_SPACE=1` ならこれだけ書く
    for (pct, k) in [(100, 1.0), (250, 2.5)] {
        let mut w = Workspace::new(
            kmb_samples::reference_document(),
            None,
            kmb_bundle::modules(),
        )
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
        w.layout_mut().appearance.text_scale = k;
        w.layout_mut().appearance.icon_scale = k;
        let mut s = Screen::new(w);
        assert!(s.click("作業空間:"));
        s.write(&dir, &format!("5c-作業空間の一覧-{pct}"));
    }
    // 名前の短い作業空間(解析)を選んだまま開いても、長い名前(アニメーション)が一覧に収まる。
    // 字の 100 %・250 % と、ハイコントラストのテーマで撮る
    for (name, k, theme) in [
        ("100", 1.0, kmb_ui::themes::STANDARD),
        ("250", 2.5, kmb_ui::themes::STANDARD),
        ("ハイコントラスト", 1.0, kmb_ui::themes::HIGH_CONTRAST),
    ] {
        let mut w = Workspace::new(
            kmb_samples::reference_document(),
            None,
            kmb_bundle::modules(),
        )
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
        w.layout_mut().appearance.text_scale = k;
        w.layout_mut().appearance.icon_scale = k;
        w.layout_mut().appearance.theme = theme.into();
        w.layout_mut().space = Space::Simulation;
        let mut s = Screen::new(w);
        assert!(s.click("作業空間:"));
        s.write(&dir, &format!("5c-作業空間の一覧-解析-{name}"));
    }
    if std::env::var("JC_SCREENS_ONLY_SPACE").is_ok() {
        return;
    }
    // `JC_SCREENS_ONLY_TOOLBAR=1` なら設計・スケッチ・加工のツールバーを明暗と字の 100 %・250 % で書く
    // (アイコンと並びを画板に合わせる作業の前後の比べ)
    if std::env::var("JC_SCREENS_ONLY_TOOLBAR").is_ok() {
        toolbar_screens(&dir);
        return;
    }
    // `JC_SCREENS_ONLY_MENUS=1` なら設計・加工・スケッチ・3D スケッチのツールバーの ▾ の一覧を明暗で書く
    // (画板に絵の無い道具に役の色を付ける作業の前後の比べ)
    if std::env::var("JC_SCREENS_ONLY_MENUS").is_ok() {
        menu_screens(&dir);
        return;
    }
    // `JC_SCREENS_ONLY_MACHINE=1` なら機械・高さの補正・レーザーの画面(1・73・90)だけ書く
    if std::env::var("JC_SCREENS_ONLY_MACHINE").is_ok() {
        machine_screens(&dir);
        return;
    }
    // `JC_SCREENS_ONLY_SETTINGS=1` なら設定の窓とキー・色覚のテーマ(70〜)だけ書く
    if std::env::var("JC_SCREENS_ONLY_SETTINGS").is_ok() {
        settings_screens(&dir);
        return;
    }
    // 0. 設計の作業空間(基準部品。リボン・ブラウザ・タイムラインの大きさと位置を試作と比べる)
    let w = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_editor(kmb_ui_sketch::SketchEditor::new())
    .with_editor(kmb_ui_tools::ToolsEditor::new());
    let mut s = Screen::new(w);
    s.write(&dir, "0-設計");
    // 1. 加工の作業空間でセットアップを選んだ所(段の帯は作業空間の下)
    let mut w = ws(new);
    let setup = first_of(&w, "cam.setup@1");
    w.select(setup);
    let mut s = Screen::new(w);
    s.write(&dir, "1-加工-セットアップ");
    // 2. 段の帯で「3 ポケット」を押した所
    if s.click("3 ポケット") {
        s.write(&dir, "2-段の帯-ポケットの後");
    }
    // 3. 輪郭の編集パネル(削る向き、入りと抜けの円弧)
    let mut w = ws(new);
    let contour = first_of(&w, "cam.contour2d@1");
    w.select(contour);
    let mut s = Screen::new(w);
    s.write(&dir, "3-輪郭の設定");
    // 4. 工具ライブラリ(テーパー、突き出し、ホルダーの径)
    let mut w = ws(new);
    let tools = first_of(&w, "cam.tools@2");
    w.select(tools);
    let mut s = Screen::new(w);
    // テーパーの工具は `new` の文書にだけある(`document`)
    if new {
        assert!(s.click("テーパー 0.5 mm"));
    }
    s.write(&dir, "4-工具ライブラリ");
    // 5. スケッチの編集(閉じた形の塗り、寸法の引出し線、足した作図の道具の見本)
    let mut w = Workspace::new(
        kmb_samples::sketch_tools_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_editor(kmb_ui_sketch::SketchEditor::new())
    .with_editor(kmb_ui_tools::ToolsEditor::new());
    let sketch = first_of(&w, "cad.sketch@1");
    w.open_edit(sketch);
    let mut s = Screen::new(w);
    s.write(&dir, "5-スケッチ-作図の見本");
    // 6. 基準部品のスケッチ(パラメータの寸法「幅 = 100」の出方を試作と比べる)
    let mut w = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_editor(kmb_ui_sketch::SketchEditor::new())
    .with_editor(kmb_ui_tools::ToolsEditor::new());
    let sketch = first_of(&w, "cad.sketch@1");
    w.open_edit(sketch);
    let mut s = Screen::new(w);
    s.write(&dir, "6-スケッチ-基準部品");
    // 7. 設計の 3D の表示: 透視投影、ブラウザの目で隠したスケッチ(薄い行と閉じた目)、
    //    下の帯の「表示 ▾」のメニュー。続けて「ワイヤフレーム」を選んだ所
    let mut w = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_editor(kmb_ui_sketch::SketchEditor::new())
    .with_editor(kmb_ui_tools::ToolsEditor::new());
    w.layout_mut().projection = kmb_ui::Projection::Perspective;
    let sketch = first_of(&w, "cad.sketch@1");
    w.set_hidden(sketch, true);
    let mut s = Screen::new(w);
    assert!(s.click_exact("表示"));
    s.write(&dir, "7-表示-透視投影と目");
    assert!(s.click("ワイヤフレーム"));
    s.write(&dir, "7-表示-線だけ");
    // 8. 3D の見本にペンシルを足し、選んだ所(内側の角の経路と、谷の角度・円弧のあてはめの欄)
    let mut w = Workspace::new(pencil_document(), None, kmb_bundle::modules())
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
    w.layout_mut().space = Space::Machining;
    let pencil = first_of(&w, "cam.pencil3d@1");
    w.select(pencil);
    let mut s = Screen::new(w);
    s.write(&dir, "8-ペンシル");
    // 9. ツールバーの 3D のまとまりの一覧(ペンシルはここから選ぶ)
    if s.click_exact("3D ▾") {
        s.write(&dir, "9-3Dの一覧");
    }
    // 10. 丸めの後の面取り(面取りが丸めの端を通り抜けて削り終わるか)
    let design = |doc| {
        Workspace::new(doc, None, kmb_bundle::modules())
            .with_editor(kmb_ui_sketch::SketchEditor::new())
            .with_editor(kmb_ui_tools::ToolsEditor::new())
    };
    let mut s = Screen::new(design(kmb_samples::fillet_then_chamfer_document()));
    s.write(&dir, "10-丸めの後の面取り");
    // 11〜14. 3 本の辺が集まる角の形。最後に選んだ機能の編集パネル(角の形の欄)
    for (type_id, corner, name) in [
        ("solid.fillet@1", "miter", "11-角-フィレット-交わり"),
        ("solid.fillet@1", "ball", "12-角-フィレット-球"),
        ("solid.chamfer@1", "miter", "13-角-面取り-交わり"),
        ("solid.chamfer@1", "patch", "14-角-面取り-三角の面"),
    ] {
        let mut s = Screen::new(design(kmb_samples::corner_document(type_id, corner)));
        s.write(&dir, name);
    }
    let mut w = design(kmb_samples::corner_document("solid.fillet@1", "ball"));
    let fillet = first_of(&w, "solid.fillet@1");
    w.select(fillet);
    let mut s = Screen::new(w);
    s.write(&dir, "15-角-フィレットの編集パネル");
    // 16. パラメータの一覧(使っていない物だけ「削除」が押せる)
    let mut w = design(kmb_samples::reference_document());
    w.add_param("extra", "1 mm");
    let mut s = Screen::new(w);
    // 「パラメータ」はリボンとブラウザの両方にあるので、最初の物(リボン)を押す
    if let Some(n) = s.h.query_all_by_label("パラメータ").next() {
        n.click();
    }
    s.settle();
    s.write(&dir, "16-パラメータの一覧と消す");
    // 17. 機能の名前を変える窓(右クリックの「名前変更」)
    let mut w = design(kmb_samples::reference_document());
    let plate = w.document().data().features[1].id;
    w.select(plate);
    w.start_rename_feature(plate);
    let mut s = Screen::new(w);
    s.write(&dir, "17-名前を変更");
    // 18. ボディの名前を変えた後のブラウザ(「ボディ1」の代わりに付けた名前)と、ボディの右クリック
    let mut w = design(kmb_samples::reference_document());
    let (last, _) = w.visible_bodies()[0];
    let bodies = vec![w.document().data().feature(last).unwrap().clone()];
    let root = w.body_numbers(&bodies)[0].1;
    w.rename_body(root, "土台");
    let mut s = Screen::new(w);
    // ブラウザの行(3D のビューの札にも同じ名前が出るので、最初の物)
    if let Some(n) = s.h.query_all_by_label_contains("土台").next() {
        n.click_secondary();
    }
    s.settle();
    s.write(&dir, "18-ボディの名前と右クリック");
    // 19〜24. テーマ(U38)。既定の見た目は上の場面のまま変わらず、選んだ時だけ替わる
    use kmb_ui::layout::{Appearance, Brightness, Corners, Density};
    let themed = |doc, cam: bool, b: Brightness, a: Appearance| {
        let mut w = design(doc);
        if cam {
            w.layout_mut().space = Space::Machining;
        }
        w.layout_mut().brightness = b;
        w.layout_mut().appearance = a;
        w
    };
    let hc = || Appearance {
        theme: kmb_ui::themes::HIGH_CONTRAST.into(),
        ..Appearance::default()
    };
    let mut s = Screen::new(themed(
        kmb_samples::reference_document(),
        false,
        Brightness::Light,
        hc(),
    ));
    s.write(&dir, "19-テーマ-ハイコントラスト-明るい");
    let mut w = themed(reference_cam_document(), true, Brightness::Dark, hc());
    let setup = first_of(&w, "cam.setup@1");
    w.select(setup);
    let mut s = Screen::new(w);
    s.write(&dir, "20-テーマ-ハイコントラスト-暗い-加工");
    let mut s = Screen::new(themed(
        kmb_samples::reference_document(),
        false,
        Brightness::Light,
        Appearance {
            text_scale: 1.5,
            icon_scale: 1.5,
            ..Appearance::default()
        },
    ));
    s.write(&dir, "21-大きさ-字とアイコン150");
    let mut s = Screen::new(themed(
        kmb_samples::reference_document(),
        false,
        Brightness::Light,
        Appearance {
            corners: Corners::Square,
            density: Density::Compact,
            text_scale: 0.8,
            icon_scale: 0.8,
            ..Appearance::default()
        },
    ));
    s.write(&dir, "22-角は四角-詰める-字80");
    let mut s = Screen::new(themed(
        kmb_samples::reference_document(),
        false,
        Brightness::Light,
        Appearance::default(),
    ));
    if let Some(n) = s.h.query_all_by_label("設定").next() {
        n.click();
    }
    s.settle();
    s.write(&dir, "23-設定の画面の節");
    let mut w = Workspace::new(
        kmb_samples::sketch_tools_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_editor(kmb_ui_sketch::SketchEditor::new())
    .with_editor(kmb_ui_tools::ToolsEditor::new());
    w.layout_mut().brightness = Brightness::Dark;
    w.layout_mut().appearance = hc();
    let sketch = first_of(&w, "cad.sketch@1");
    w.open_edit(sketch);
    let mut s = Screen::new(w);
    s.write(&dir, "24-テーマ-ハイコントラスト-暗い-スケッチ");
    // 30. パラメータの表(U50): ボディの右クリックで作った表(ボディに関わる物と機能の値だけ)
    let mut w = design(kmb_samples::reference_document());
    let (last, _) = w.visible_bodies()[0];
    let bodies = vec![w.document().data().feature(last).unwrap().clone()];
    let root = w.body_numbers(&bodies)[0].1;
    w.make_body_param_view(root);
    let mut s = Screen::new(w);
    s.settle();
    s.write(&dir, "30-パラメータの表-ボディ1に関わる物");
    // 31. 「全部」の表で行の名前を右クリック →「表に入れる」
    if let Some(n) = s.h.query_all_by_label("全部").next() {
        n.click();
    }
    s.settle();
    if let Some(n) = s.h.query_all_by_value("hole").next() {
        n.click_secondary();
    }
    s.settle();
    if let Some(n) = s.h.query_all_by_label_contains("表に入れる").next() {
        n.hover();
    }
    s.settle();
    s.write(&dir, "31-行の右クリック-表に入れる");
    // 32〜34. 押し出し・穴・回転の編集パネル
    let mut w = design(kmb_samples::reference_document());
    let boss = w
        .document()
        .data()
        .features
        .iter()
        .find(|f| f.name == "ボス")
        .unwrap()
        .id;
    w.select(boss);
    let mut s = Screen::new(w);
    s.write(&dir, "32-押し出しの編集パネル");
    let mut w = design(kmb_samples::reference_document());
    let hole = first_of(&w, "solid.hole@1");
    w.select(hole);
    let mut s = Screen::new(w);
    s.write(&dir, "33-穴の編集パネル");
    let mut w = design(revolve_document());
    let rev = first_of(&w, "solid.revolve@1");
    w.select(rev);
    let mut s = Screen::new(w);
    s.write(&dir, "34-回転の編集パネル");
    settings_screens(&dir);
    // 人の手順で通しで確かめる時に開く文書(アプリで開いて、回転・薄い押し出しを作る)
    for (name, doc) in [
        ("回転の輪郭", revolve_document()),
        ("線だけのスケッチ", lines_document()),
    ] {
        let mut doc = doc;
        // 文書の中の回転は消して、輪郭だけにする
        let ids: Vec<_> = doc
            .data()
            .features
            .iter()
            .filter(|f| f.type_id != "cad.sketch@1")
            .map(|f| f.id)
            .collect();
        for id in ids {
            doc.apply(kmb_core::Transaction::new(
                "削除",
                vec![kmb_core::Op::RemoveFeature { id }],
            ))
            .unwrap();
        }
        std::fs::write(format!("{dir}/{name}.json"), doc.to_json()).unwrap();
    }
}

/// 70〜81. 設定の窓の頁と検索(U39)、キーの割り当て(U36)、字とアイコンの 50〜250 %、色覚の型のテーマ(U27)。
fn settings_screens(dir: &str) {
    use kmb_ui::layout::{Appearance, Brightness};
    let full = |doc| {
        let mut w = Workspace::new(doc, None, kmb_bundle::modules())
            .with_editor(kmb_ui_sketch::SketchEditor::new())
            .with_editor(kmb_ui_tools::ToolsEditor::new());
        w.layout_mut().brightness = Brightness::Light;
        w
    };
    // 窓は画面を作った後に開く(作る時の「全体」を押す手が、窓の中の「全体表示」と重ならない)
    let settings = |w: Workspace, page: &str, name: &str| {
        let mut s = Screen::new(w);
        s.h.state_mut().open_settings_at(page);
        s.settle();
        s.write(dir, name);
    };
    settings(
        full(kmb_samples::reference_document()),
        "general/appearance",
        "70-設定-一般-画面",
    );
    settings(
        full(kmb_samples::reference_document()),
        "general/input",
        "71-設定-マウスとキー",
    );
    // 押し出しと穴に同じ W を付けると、両方の行に印が出る
    let mut w = full(kmb_samples::reference_document());
    w.layout_mut()
        .keys
        .insert("extrude".into(), vec!["W".into()]);
    w.layout_mut().keys.insert("hole".into(), vec!["W".into()]);
    w.layout_mut().keys.insert("measure".into(), vec![]);
    settings(w, "general/input", "72-設定-キーの重なりと外した行");
    settings(
        full(kmb_samples::reference_cam_document()),
        "machining/cam.machine",
        "73-設定-加工-機械",
    );
    settings(
        full(kmb_samples::reference_document()),
        "design/sketch.display",
        "81-設定-設計-スケッチの寸法の表示",
    );
    let mut s = Screen::new(full(kmb_samples::reference_document()));
    s.h.state_mut().set_settings_query("投影");
    s.settle();
    s.write(dir, "74-設定-検索-投影");
    for (scale, name) in [
        (2.5, "75-大きさ-字とアイコン250"),
        (0.5, "76-大きさ-字とアイコン50"),
    ] {
        let mut w = full(kmb_samples::reference_document());
        w.layout_mut().appearance = Appearance {
            text_scale: scale,
            icon_scale: scale,
            ..Appearance::default()
        };
        let mut s = Screen::new(w);
        s.settle();
        s.write(dir, name);
    }
    for (theme, b, doc, page, name) in [
        (
            kmb_ui::themes::CVD_RG,
            Brightness::Light,
            kmb_samples::reference_document(),
            Some("general/appearance"),
            "77-テーマ-色覚12型-明るい-見本",
        ),
        (
            kmb_ui::themes::CVD_RG,
            Brightness::Dark,
            kmb_samples::reference_cam_document(),
            None,
            "78-テーマ-色覚12型-暗い-加工",
        ),
        (
            kmb_ui::themes::CVD_TRI,
            Brightness::Light,
            kmb_samples::reference_cam_document(),
            None,
            "79-テーマ-色覚3型-明るい-加工",
        ),
        (
            kmb_ui::themes::CVD_TRI,
            Brightness::Dark,
            kmb_samples::reference_document(),
            Some("general/appearance"),
            "80-テーマ-色覚3型-暗い-見本",
        ),
    ] {
        let cam = page.is_none();
        let mut w = full(doc);
        w.layout_mut().brightness = b;
        w.layout_mut().appearance.theme = theme.into();
        if cam {
            w.layout_mut().space = Space::Machining;
            let setup = first_of(&w, "cam.setup@1");
            w.select(setup);
        }
        let mut s = Screen::new(w);
        if let Some(p) = page {
            s.h.state_mut().open_settings_at(p);
        }
        s.settle();
        s.write(dir, name);
    }
}

/// 機械を選ぶ欄と高さの補正の欄(セットアップ)、設定の機械、レーザーの加工の欄。
/// 設計・スケッチ・加工のツールバー(明暗 × 字とアイコンの 100 %・250 %)。
fn toolbar_screens(dir: &str) {
    use kmb_ui::layout::Brightness;
    for (b, bn) in [(Brightness::Dark, "暗"), (Brightness::Light, "明")] {
        for (pct, k) in [(100, 1.0), (250, 2.5)] {
            for place in ["設計", "スケッチ", "加工"] {
                let doc = if place == "加工" {
                    reference_cam_document()
                } else {
                    kmb_samples::reference_document()
                };
                let mut w = Workspace::new(doc, None, kmb_bundle::modules())
                    .with_editor(kmb_ui_sketch::SketchEditor::new())
                    .with_editor(kmb_ui_tools::ToolsEditor::new());
                w.layout_mut().brightness = b;
                w.layout_mut().appearance.text_scale = k;
                w.layout_mut().appearance.icon_scale = k;
                match place {
                    "加工" => w.layout_mut().space = Space::Machining,
                    "スケッチ" => {
                        let sk = first_of(&w, "cad.sketch@1");
                        w.open_edit(sk);
                    }
                    _ => {}
                }
                let mut s = Screen::new(w);
                s.write(dir, &format!("tb-{place}-{bn}-{pct}"));
            }
        }
    }
}

fn menu_screens(dir: &str) {
    use kmb_ui::layout::Brightness;
    let places: [(&str, &[&str]); 4] = [
        (
            "設計",
            &[
                "作成",
                "修正",
                "パターン",
                "組立",
                "検査",
                "挿入",
                "書き出し",
            ],
        ),
        (
            "加工",
            &[
                "セットアップ",
                "2D",
                "3D",
                "穴",
                "確かめる",
                "NC プログラム",
                "出力",
                "管理",
            ],
        ),
        ("スケッチ", &["作成", "修正", "拘束", "寸法"]),
        (
            "3Dスケッチ",
            &["", "3D 作成", "作図面", "取り込み", "3D 拘束", "3D 寸法"],
        ),
    ];
    for (b, bn) in [(Brightness::Dark, "暗"), (Brightness::Light, "明")] {
        for (place, groups) in places {
            for g in groups {
                let doc = if place == "加工" {
                    reference_cam_document()
                } else {
                    kmb_samples::reference_document()
                };
                let mut w = Workspace::new(doc, None, kmb_bundle::modules())
                    .with_editor(kmb_ui_sketch::SketchEditor::new())
                    .with_editor(kmb_ui_sketch::Sketch3dEditor::new())
                    .with_editor(kmb_ui_tools::ToolsEditor::new());
                w.layout_mut().brightness = b;
                match place {
                    "加工" => w.layout_mut().space = Space::Machining,
                    "スケッチ" => {
                        let sk = first_of(&w, "cad.sketch@1");
                        w.open_edit(sk);
                    }
                    "3Dスケッチ" => {
                        w.run_action("cad.add.sketch3d");
                    }
                    _ => {}
                }
                let mut s = Screen::new(w);
                // 名前の無い物は帯だけ(3D スケッチは前のボタンも画板に無いので、帯も撮る)
                if g.is_empty() {
                    s.write(dir, &format!("menu-{place}-{bn}-帯"));
                } else if s.click_exact(&format!("{g} ▾")) {
                    s.write(dir, &format!("menu-{place}-{bn}-{g}"));
                }
            }
        }
    }
}

fn machine_screens(dir: &str) {
    let open = |doc| {
        let mut w = Workspace::new(doc, None, kmb_bundle::modules())
            .with_editor(kmb_ui_sketch::SketchEditor::new())
            .with_editor(kmb_ui_tools::ToolsEditor::new());
        w.layout_mut().space = Space::Machining;
        w
    };
    let mut w = open(document(false));
    let setup = first_of(&w, "cam.setup@1");
    w.select(setup);
    Screen::new(w).write(dir, "1-加工-セットアップ");
    // 汎用の Grbl 系を選び、G-code の先頭で高さを測る(補正の命令の無い機械なので注意が出る)
    let mut doc = document(false);
    let id = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == "cam.setup@1")
        .unwrap()
        .id;
    let mut data = doc.data().feature(id).unwrap().data.clone();
    data["machine"] = "generic-grbl".into();
    data["leveling"] = serde_json::json!({"mode": "gcode", "nx": 5, "ny": 4});
    doc.apply(kmb_core::Transaction::new(
        "機械",
        vec![kmb_core::Op::SetFeatureData { id, data }],
    ))
    .unwrap();
    let mut w = open(doc);
    w.select(id);
    Screen::new(w).write(dir, "1b-加工-セットアップ-汎用と高さの補正");
    let mut s = Screen::new(open(reference_cam_document()));
    s.h.state_mut().open_settings_at("machining/cam.machine");
    s.settle();
    s.write(dir, "73-設定-加工-機械");
    let mut w = open(laser_document());
    let laser = first_of(&w, "cam.laser@1");
    w.select(laser);
    Screen::new(w).write(dir, "90-加工-レーザー");
    // 線で切り抜く: 切り口の補正(部品を残す)、円弧の進入、直線の退出、重ね切り、貫通
    let mut doc = laser_document();
    let id = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == "cam.laser@1")
        .unwrap()
        .id;
    let mut data = doc.data().feature(id).unwrap().data.clone();
    for (k, v) in [
        ("mode", serde_json::json!("line")),
        ("kerf_side", serde_json::json!("outside")),
        ("kerf", serde_json::json!("0.1 mm")),
        ("lead_in", serde_json::json!("arc")),
        ("lead_in_sweep", serde_json::json!("120 deg")),
        ("lead_in_line", serde_json::json!("1 mm")),
        ("lead_out", serde_json::json!("line")),
        ("overcut", serde_json::json!("0.5 mm")),
        ("pierce_time", serde_json::json!(0.3)),
        ("pierce_clearance", serde_json::json!("0.5 mm")),
    ] {
        data[k] = v;
    }
    doc.apply(kmb_core::Transaction::new(
        "切り抜き",
        vec![kmb_core::Op::SetFeatureData { id, data }],
    ))
    .unwrap();
    let mut w = open(doc);
    w.select(id);
    Screen::new(w).write(dir, "90b-加工-レーザー-切り抜き");
}

/// 基準部品の加工の文書に、レーザーの工具(T9)と、最初のスケッチの中を塗るレーザーの加工を足す。
fn laser_document() -> kmb_core::Document {
    let mut doc = reference_cam_document();
    let of = |doc: &kmb_core::Document, t: &str| {
        doc.data()
            .features
            .iter()
            .find(|f| f.type_id == t)
            .unwrap()
            .id
    };
    let (setup, tools, sketch) = (
        of(&doc, "cam.setup@1"),
        of(&doc, "cam.tools@2"),
        of(&doc, "cad.sketch@1"),
    );
    let mut data = doc.data().feature(tools).unwrap().data.clone();
    let mut t = data["tools"][0].clone();
    t["number"] = 9.into();
    t["name"] = "レーザー".into();
    t["kind"] = "laser".into();
    t["diameter"] = 0.1.into();
    data["tools"].as_array_mut().unwrap().push(t);
    doc.apply(kmb_core::Transaction::new(
        "工具を足す",
        vec![kmb_core::Op::SetFeatureData { id: tools, data }],
    ))
    .unwrap();
    let id = doc.alloc_feature_id();
    let data = serde_json::json!({
        "setup": setup, "tools": tools, "tool": 9, "curves": sketch, "mode": "fill",
        "rpm": "40", "feed": "1500", "passes": 1, "focus": "0 mm", "interval": "0.1 mm",
        "angle": "0 deg", "both_ways": true, "cross": false,
    });
    doc.apply(kmb_core::Transaction::new(
        "レーザーを足す",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: "cam.laser@1".into(),
                name: "焼き印".into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    doc
}

/// 見本の部品に、Z に立てた線 1 本(点 (-30, 0, 0) から (-30, 0, 30)、線の番号 3)の 3D スケッチを
/// 足した文書。
fn sketch3d_line_document() -> kmb_core::Document {
    let mut doc = kmb_samples::reference_document();
    let id = doc.alloc_feature_id();
    let data = serde_json::json!({"entities": [
        {"type": "point", "id": 1, "p": [-30.0, 0.0, 0.0]},
        {"type": "point", "id": 2, "p": [-30.0, 0.0, 30.0]},
        {"type": "line", "id": 3, "a": 1, "b": 2},
    ]});
    doc.apply(kmb_core::Transaction::new(
        "3D スケッチを足す",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: "cad.sketch3d@1".into(),
                name: "3D スケッチ1".into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    doc
}

/// XY 平面の L 字の線(閉じていない)だけのスケッチの文書。
fn lines_document() -> kmb_core::Document {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::xy();
    let p0 = s.fixed_point(0.0, 0.0);
    let p1 = s.fixed_point(40.0, 0.0);
    let p2 = s.fixed_point(40.0, 25.0);
    s.line(p0, p1);
    s.line(p1, p2);
    b.feature("cad.sketch@1", "線", s.build());
    b.finish("線")
}

/// ZX 平面の長方形(世界の x 10〜15、z 0〜8)を、原点の Z 軸まわりに 270 度回した文書。
fn revolve_document() -> kmb_core::Document {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let mut b = DocBuilder::new();
    let mut p = SketchBuilder::zx();
    let o = p.fixed_point(0.0, 0.0);
    let c = p.point_from(o, "0 mm", "10 mm", [0.0, 10.0]);
    p.rectangle(c, "8 mm", "5 mm", [0.0, 10.0, 8.0, 5.0]);
    let profile = b.feature("cad.sketch@1", "輪郭", p.build());
    b.feature(
        "solid.revolve@1",
        "回転",
        serde_json::json!({"profile": profile, "axis": {"kind": "origin_z"}, "angle": "270 deg"}),
    );
    b.finish("回転")
}

/// 3D の見本の最後に、ボール 3.175 mm(4 番)のペンシルを足した文書。
fn pencil_document() -> kmb_core::Document {
    let mut doc = kmb_samples::reference_cam_3d_document();
    let of = |doc: &kmb_core::Document, t: &str| {
        doc.data()
            .features
            .iter()
            .find(|f| f.type_id == t)
            .unwrap()
            .id
    };
    let (setup, tools) = (of(&doc, "cam.setup@1"), of(&doc, "cam.tools@2"));
    let id = doc.alloc_feature_id();
    let data = serde_json::json!({
        "setup": setup, "tools": tools, "tool": 4, "fit": "0.005 mm",
        "leave": "0 mm", "resolution": "0.05 mm",
    });
    doc.apply(kmb_core::Transaction::new(
        "ペンシルを足す",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: "cam.pencil3d@1".into(),
                name: "内側の角".into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    doc
}

/// 24 ビットの BMP を書く。
fn write_bmp(path: &str, img: &ColorImage) {
    let [w, h] = img.size;
    let row = (w * 3).div_ceil(4) * 4;
    let mut out = Vec::with_capacity(54 + row * h);
    let size = (54 + row * h) as u32;
    out.extend(b"BM");
    out.extend(size.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((w as i32).to_le_bytes());
    out.extend((h as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend([0u8; 24]);
    for y in (0..h).rev() {
        let start = out.len();
        for x in 0..w {
            let c = img.pixels[y * w + x];
            out.extend([c.b(), c.g(), c.r()]);
        }
        out.resize(start + row, 0);
    }
    std::fs::write(path, out).unwrap();
}

/// ツールバーと上の帯の場面: 字とアイコンが 100 %・250 % の
/// 設計を、幅 1920 と 1280 で。250 % では上の帯の印「jc」と知らせの鐘が隣と重ならないこと、
/// 帯が畳まれて「選択」まで画面に入ることを見る。`JC_SCREENS_OUT` に書く。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_toolbar_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    for (pct, k) in [(100, 1.0), (250, 2.5)] {
        for width in [1920.0, 1280.0] {
            for labels in [true, false] {
                let mut w = Workspace::new(
                    kmb_samples::reference_document(),
                    None,
                    kmb_bundle::modules(),
                )
                .with_editor(kmb_ui_sketch::SketchEditor::new())
                .with_editor(kmb_ui_tools::ToolsEditor::new());
                w.layout_mut().appearance.text_scale = k;
                w.layout_mut().appearance.icon_scale = k;
                w.layout_mut().appearance.tool_labels = labels;
                let mut s = Screen::sized(w, Some([width, 800.0]));
                let name = if labels {
                    "名前あり"
                } else {
                    "名前なし"
                };
                s.write(&dir, &format!("ツールバー-{pct}-{width}-{name}"));
            }
        }
    }
}

/// タイムライン・ファイル・文書の場面。`JC_SCREENS_OUT` に書く。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_doc_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let design = |doc| {
        Workspace::new(doc, None, kmb_bundle::modules())
            .with_editor(kmb_ui_sketch::SketchEditor::new())
            .with_editor(kmb_ui_tools::ToolsEditor::new())
    };
    // 40〜41. ホームの最近使ったデザイン(U15): ピン留めした物、見つからない物、カードの右クリック
    let tmp = std::env::temp_dir().join(format!("kmb-screens-doc-{}", std::process::id()));
    std::fs::remove_dir_all(&tmp).ok();
    let data = tmp.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let files: Vec<_> = ["古い試作", "ふた", "台座"]
        .iter()
        .map(|n| tmp.join(format!("{n}.json")))
        .collect();
    let mut w = design(kmb_samples::sample_document()).with_data_dir(data.clone());
    for f in &files {
        kmb_session::save(f, &kmb_samples::reference_document()).unwrap();
        assert!(w.load_path(f));
    }
    std::fs::remove_file(&files[0]).unwrap();
    w.recent_action(&files[1], kmb_ui::RecentAction::Pin(true));
    let w = design(kmb_samples::sample_document())
        .with_home(kmb_ui_library::Library::new())
        .with_data_dir(data.clone())
        .show_start();
    let mut s = Screen::new(w);
    s.write(&dir, "40-ホーム-最近使ったデザイン");
    if let Some(n) = s.h.query_all_by_label_contains("古い試作").next() {
        n.click_secondary();
    }
    s.settle();
    s.write(&dir, "41-ホーム-カードの右クリック");
    // 42〜43. ブラウザの「名前の付いたビュー」(U20)と、文書の右クリック(設計の属性)
    let mut w = design(kmb_samples::reference_document());
    let mut p = kmb_core::Properties::default();
    p.set("title", "台座");
    p.set("part_number", "JC-001");
    p.set("material", "A5052");
    w.set_properties(p);
    w.save_named_view("正面から");
    w.save_named_view("");
    let mut s = Screen::new(w);
    assert!(s.click_exact("名前の付いたビュー"));
    s.write(&dir, "42-ブラウザ-名前付きの視点");
    // ブラウザの一番上の行(題の「(無題)」と同じ名前なので、左の端にある物)
    if let Some(n) =
        s.h.query_all_by_label_contains("(無題)")
            .find(|n| n.rect().left() < 260.0)
    {
        n.click_secondary();
    }
    s.settle();
    s.write(&dir, "43-文書の右クリック");
    // 44. 設計の属性の窓(U13)
    assert!(s.click("設計の属性"));
    s.write(&dir, "44-設計の属性");
    // 45. STEP を読み込んだすぐ後の「立てるか」(U12)
    let step = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/kmb-kernel-truck/testdata/reference.step");
    let mut w = design(kmb_core::Document::new());
    // kmb_cad_import_step::IMPORT_STEP_FILE
    w.run_action_with("cad.import_step", Some(step.clone()));
    let mut s = Screen::new(w);
    s.write(&dir, "45-STEPを立てるか");
    assert!(s.click_exact("立てる"));
    assert!(s.click("全体表示"));
    s.write(&dir, "46-STEPを立てた後");
    // 47. 計算に失敗した間は前の形を出したまま知らせる(M27)
    let part = kmb_samples::reference_part();
    let mut w = design(part.doc);
    let mut d = w
        .document()
        .data()
        .feature(part.pocket)
        .unwrap()
        .data
        .clone();
    d["distance"] = serde_json::json!("0 mm");
    w.set_feature_data(part.pocket, d);
    let mut s = Screen::new(w);
    s.write(&dir, "47-計算に失敗-前の形");
    // 48〜49. 機能の右クリックのコピーと貼り付け(U42)、貼った後の選び直し(丸めの辺を選び直す)
    let mut w = design(kmb_samples::fillet_then_chamfer_document());
    let fillet = first_of(&w, "solid.fillet@1");
    let name = w.document().data().feature(fillet).unwrap().name.clone();
    w.select(fillet);
    let mut s = Screen::new(w);
    if let Some(n) = s.h.query_all_by_label(&name).next() {
        n.click_secondary();
    }
    s.settle();
    s.write(&dir, "48-機能の右クリック-コピー");
    assert!(s.click("コピー("));
    if let Some(n) = s.h.query_all_by_label(&name).next() {
        n.click_secondary();
    }
    s.settle();
    assert!(s.click("貼り付け("));
    s.write(&dir, "49-貼り付けた後-選び直し");
    // 50. 大きなファイルを開く途中(U8): 進み具合とやめる。前の形は出したまま
    let big = tmp.join("3D の仕上げ.json");
    kmb_session::save(&big, &kmb_samples::reference_cam_3d_document()).unwrap();
    let w = Workspace::open(
        kmb_samples::reference_document(),
        None,
        true,
        kmb_bundle::modules(),
    )
    .with_editor(kmb_ui_sketch::SketchEditor::new())
    .with_editor(kmb_ui_tools::ToolsEditor::new());
    let mut s = Screen::new(w);
    s.h.state_mut().wait();
    s.settle();
    s.h.state_mut().load_path(&big);
    std::thread::sleep(std::time::Duration::from_millis(600));
    s.settle();
    s.write(&dir, "50-開く途中-進み具合とやめる");
    std::fs::remove_dir_all(&tmp).ok();
}

/// 30×20×10 の板を厚みの半分で 2 つのボディに割った画面(断面の色分けと分割の境の線を見る)。
fn split_plate() -> Workspace {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::xy();
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "30 mm", "20 mm", [0.0, 0.0, 30.0, 20.0]);
    let sk = b.sketch("板のスケッチ", s);
    let plate = b.feature(
        "solid.extrude@1",
        "板",
        serde_json::json!({"profile": sk, "distance": "10 mm"}),
    );
    let mut ws = Workspace::new(b.finish("分割した板"), None, kmb_bundle::modules())
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
    ws.wait();
    ws.select(plate);
    ws.add_described("solid.split@1").expect("分割を足せる");
    ws.wait();
    ws.finish_edit();
    ws.wait();
    ws.clear_selection();
    ws
}

/// 押し出しの高さを決める間に、マウスを柱の上の面に載せた所を撮る(その面まで伸ばす見本と
/// 「〇〇まで」の字)。マウスの位置は案内の「(面まで)」が出る所を探す。`JC_SHOT_AT=x,y` で
/// 位置を決めれば探さない(直す前の版で同じ所を撮る時)。`JC_SCREENS_OUT` に書く。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_extrude_to_face_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let mut b = DocBuilder::new();
    let rect = |x: f64, y: f64| {
        let mut s = SketchBuilder::xy();
        let o = s.fixed_point(0.0, 0.0);
        let c = s.point_from(o, &format!("{x} mm"), &format!("{y} mm"), [x, y]);
        s.rectangle(c, "6 mm", "6 mm", [x, y, 6.0, 6.0]);
        s
    };
    let post = b.sketch("柱のスケッチ", rect(20.0, 0.0));
    b.feature(
        "solid.extrude@1",
        "柱",
        serde_json::json!({"profile": post, "distance": "20 mm"}),
    );
    let sketch = b.sketch("伸ばすスケッチ", rect(0.0, 0.0));
    let mut ws = Workspace::new(b.finish("柱まで伸ばす"), None, kmb_bundle::modules())
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
    ws.wait();
    ws.select(sketch);
    let mut s = Screen::new(ws);
    s.h.state_mut().set_view(-60.0, 30.0);
    s.settle();
    assert!(s.click_exact("押し出し"), "押し出しのボタンが無い");
    let given = std::env::var("JC_SHOT_AT").ok().and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some(egui::pos2(x.parse().ok()?, y.parse().ok()?))
    });
    let at = given.or_else(|| {
        (0..(s.size[1] as i32 / 8)).find_map(|j| {
            (0..(s.size[0] as i32 / 8)).find_map(|i| {
                let p = egui::pos2(i as f32 * 8.0, j as f32 * 8.0);
                s.h.event(egui::Event::PointerMoved(p));
                s.h.step();
                s.h.query_by_label_contains("(面まで)").map(|_| p)
            })
        })
    });
    let at = at.expect("柱の上の面が見つからない");
    // 面の縁から少し内へ
    let at = if given.is_some() {
        at
    } else {
        at + egui::vec2(6.0, 10.0)
    };
    println!("JC_SHOT_AT={},{}", at.x, at.y);
    s.h.event(egui::Event::PointerMoved(at));
    s.settle();
    s.write(&dir, "押し出しの高さを柱の上の面に載せる");
}

/// 90〜95. 3D のビューの表示(レビュー 2 回目の本 2): 前・右から見た時の格子(M10)、
/// 断面の切り口の縞と色(M11)、分割したボディの境の線(M8)。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_view3d_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let design = |doc| {
        Workspace::new(doc, None, kmb_bundle::modules())
            .with_editor(kmb_ui_sketch::SketchEditor::new())
            .with_editor(kmb_ui_tools::ToolsEditor::new())
    };
    // 人の手順で確かめる時に開く文書(アプリで開いて、断面・視点の向きを替える)
    std::fs::write(
        format!("{dir}/分割した板.json"),
        split_plate().document().to_json(),
    )
    .unwrap();
    if std::env::var("JC_SCREENS_ONLY_DOCS").is_ok() {
        return;
    }
    // 前から見る(-Y の軸の玉)・右から見る(X の軸の玉)
    for (name, yaw) in [("90-前から見た格子", -90.0), ("91-右から見た格子", 0.0)] {
        let mut s = Screen::new(design(kmb_samples::reference_document()));
        s.h.state_mut().set_view(yaw, 0.0);
        s.settle();
        s.write(&dir, name);
    }
    // 分割した板(斜めから。境の線が出るか)
    let mut s = Screen::new(split_plate());
    s.write(&dir, "92-分割した板の境の線");
    // 断面(前の面に平行な ZX 平面で真ん中を切る)
    let mut s = Screen::new(split_plate());
    s.h.state_mut().toggle_section();
    s.settle();
    s.write(&dir, "93-分割した板の断面");
    if s.click("外側も同じ色で塗る") {
        s.write(&dir, "94-断面で外側も同じ色");
    }
    let mut s = Screen::new(design(kmb_samples::reference_document()));
    s.h.state_mut().clear_selection();
    s.h.state_mut().toggle_section();
    s.settle();
    s.write(&dir, "95-基準部品の断面");
}

/// 写真 103: AI が変えた所。Claude が MCP で幅を変えた直後の画面で、変えた
/// 機能がタイムラインとブラウザで、そのボディが 3D のビューで光り、下の帯に道具の名前が出る。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_ai_mark_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let data = std::env::temp_dir().join(format!("kmb-ai-mark-{}", std::process::id()));
    let ws = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_data_dir(data.clone());
    let mut s = Screen::new(ws);
    s.h.state_mut().layout_mut().ai_live = true;
    s.settle();
    let d = data.clone();
    let h = std::thread::spawn(move || {
        kmb_session::live::call(
            &d,
            "set_param",
            serde_json::json!({"name": "width", "expr": "70 mm"}),
        )
    });
    while !h.is_finished() {
        s.h.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.join().unwrap().unwrap();
    s.h.step();
    s.write(&dir, "103-AIが幅を変えた直後");
    s.h.state_mut().layout_mut().ai_live = false;
    s.settle();
    let _ = std::fs::remove_dir_all(&data);
}

/// 96〜101. 絵と動画(23b・23c): 外観の材質(木目・金属)の表示と、レンダリングの作業空間の
/// キャンバス内レンダリング。96 は表示の「効果」の「材質の質感」を切った絵(前の版と同じ描き方)。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_render_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let with = |look: &str, materials: bool| {
        let mut ws = Workspace::new(
            kmb_samples::reference_document(),
            None,
            kmb_bundle::modules(),
        )
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
        let (_, root) = ws.body_roots()[0];
        ws.set_body_appearance(root, look);
        ws.layout_mut().effects.materials = materials;
        ws.clear_selection();
        ws
    };
    // 人の手順で確かめる時に開く文書(アプリで開いて、レンダリングの作業空間で絵と動画を書き出す)
    std::fs::write(
        format!("{dir}/基準部品-ウォルナット.json"),
        with("ウォルナット", true).document().to_json(),
    )
    .unwrap();
    if std::env::var("JC_SCREENS_ONLY_DOCS").is_ok() {
        return;
    }
    for (name, look, on) in [
        (
            "96-ウォルナット-材質の質感なし(前と同じ)",
            "ウォルナット",
            false,
        ),
        ("97-ウォルナット-材質の質感", "ウォルナット", true),
        ("98-アルミ-材質の質感", "アルミ", true),
    ] {
        let mut s = Screen::new(with(look, on));
        s.write(&dir, name);
    }
    // レンダリングの作業空間: 上のタブとまとまり。キャンバス内レンダリングを押して回す
    let mut s = Screen::new(with("メープル", true));
    s.h.state_mut().switch_space(kmb_ui::layout::Space::Render);
    s.settle();
    s.write(&dir, "99-レンダリングの作業空間");
    s.h.state_mut().run_item("u:canvas_render");
    let t = std::time::Instant::now();
    while s.h.state().canvas_render_passes().unwrap_or(0) < 32 && t.elapsed().as_secs() < 300 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        s.h.step();
    }
    s.settle();
    s.write(&dir, "100-キャンバス内レンダリング");
    // 暗いスタジオ
    s.h.state_mut()
        .change_settings("render.scene", &serde_json::json!({"env": "studio_dark"}));
    let t = std::time::Instant::now();
    s.settle();
    while s.h.state().canvas_render_passes().unwrap_or(0) < 32 && t.elapsed().as_secs() < 300 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        s.h.step();
    }
    s.settle();
    s.write(&dir, "101-キャンバス内レンダリング-暗いスタジオ");
}

/// 削る様子の動画(24a)の見本: 加工の基準の文書を、加工の作業空間の視点で GIF にする。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_sim_video_sample() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut ws = Workspace::new(
        kmb_samples::reference_cam_document(),
        None,
        kmb_bundle::modules(),
    );
    ws.change_settings(
        "render.scene",
        &serde_json::json!({"video_width": 640, "video_height": 400, "seconds": 4, "fps": 15}),
    );
    ws.switch_space(kmb_ui::layout::Space::Machining);
    // 画面を描いて全体に合わせた視点で撮る(人がビューを見ながら押すのと同じ)
    let mut s = Screen::new(ws);
    let out = std::path::Path::new(&dir).join("102-削る様子の動画.gif");
    s.h.state_mut()
        .run_action_with("cam.export_sim_video", Some(out.clone()));
    s.h.state_mut().wait_task();
    assert!(out.exists(), "{:?}", s.h.state().message());
}

/// 蓋つきの箱(本体 120×80×60、蓋 120×80×6、つまみ 20×20×12)。蓋とつまみは部品「蓋」で、
/// 奥の上の辺の蝶番(-110〜0 度)で本体につなぐ。段の帯: 蓋を開ける → 閉じる → 蓋を上へ分解 →
/// つまみを上へ分解。
fn lidded_box() -> Workspace {
    let mut ws = Workspace::new(lidded_box_document(), None, kmb_bundle::modules());
    ws.wait();
    ws
}

/// ジョイントで蓋が開く箱の文書(ストーリーボードの段つき)。台本の `#! sample box` も使う。
fn lidded_box_document() -> kmb_core::Document {
    lidded_box_doc(false)
}

/// [`lidded_box_document`] に、つまみを蓋の上で縦軸のまわりに回すジョイント「つまみの回転」と、
/// 本体の右に離して置いた札(20×20×6 のボディ。部品に入れない)を足した物(ジョイントが 2 つ要る
/// 台本と、剛性グループの台本に)。
fn lidded_box_with_knob_joint() -> kmb_core::Document {
    lidded_box_doc(true)
}

fn lidded_box_doc(knob_joint: bool) -> kmb_core::Document {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let mut b = DocBuilder::new();
    // (最初の機能, 最後の機能)
    let part = |b: &mut DocBuilder, name: &str, [x, y, w, d]: [f64; 4], t: f64, z: f64| {
        let mut s = SketchBuilder::xy();
        let o = s.fixed_point(x, y);
        s.rectangle(
            o,
            &format!("{w} mm"),
            &format!("{d} mm"),
            [x, y, x + w, y + d],
        );
        let sk = b.sketch(&format!("{name}のスケッチ"), s);
        let e = b.feature(
            "solid.extrude@1",
            name,
            serde_json::json!({"profile": sk, "distance": format!("{t} mm")}),
        );
        if z == 0.0 {
            return (e, e);
        }
        let m = b.feature(
            "solid.move@1",
            &format!("{name}を上げる"),
            serde_json::json!({"body": e.0, "z": format!("{z} mm")}),
        );
        (e, m)
    };
    let (body, _) = part(&mut b, "本体", [0.0, 0.0, 120.0, 80.0], 60.0, 0.0);
    let (lid, lid_up) = part(&mut b, "蓋", [0.0, 0.0, 120.0, 80.0], 6.0, 60.0);
    let (knob, knob_up) = part(&mut b, "つまみ", [50.0, 30.0, 20.0, 20.0], 12.0, 66.0);
    if knob_joint {
        part(&mut b, "札", [140.0, 0.0, 20.0, 20.0], 6.0, 0.0);
    }
    let joint = b.feature(
        "asm.joint@1",
        "蝶番",
        serde_json::json!({
            "kind": "revolute",
            "moving": {"kind": "body", "root": lid.0},
            "base": {"kind": "body", "root": body.0},
            "bodies": [lid_up.0, knob_up.0],
            "origin": [0.0, 80.0, 66.0],
            "axis": [1.0, 0.0, 0.0],
            "angle_min": "-110 deg",
            "angle_max": "0 deg",
        }),
    );
    if knob_joint {
        b.feature(
            "asm.joint@1",
            "つまみの回転",
            serde_json::json!({
                "kind": "revolute",
                "moving": {"kind": "body", "root": knob.0},
                "base": {"kind": "body", "root": lid.0},
                "bodies": [knob_up.0],
                "origin": [60.0, 40.0, 66.0],
                "axis": [0.0, 0.0, 1.0],
                "follows": [joint.0],
            }),
        );
    }
    let mut doc = b.finish("蓋つきの箱");
    let c = doc.alloc_group_id();
    let d = doc.alloc_group_id();
    doc.apply(kmb_core::Transaction::new(
        "部品",
        vec![kmb_core::Op::SetComponents {
            components: vec![
                kmb_core::Component {
                    id: c,
                    name: "本体".into(),
                    parent: None,
                    bodies: vec![body],
                },
                kmb_core::Component {
                    id: d,
                    name: "蓋".into(),
                    parent: None,
                    bodies: vec![lid, knob],
                },
            ],
        }],
    ))
    .unwrap();
    let mut looks = kmb_look::appearance::Appearances::default();
    looks.set_component(c, Some("ウォルナット"));
    looks.set_component(d, Some("メープル"));
    let op = doc.data().section_op(&looks).unwrap();
    doc.apply(kmb_core::Transaction::new("外観", vec![op]))
        .unwrap();
    use kmb_interchange::animation::{Step, StepKind, Target};
    let drive = |from: f64, to: f64, seconds: f64| Step {
        kind: StepKind::Drive { joint, from, to },
        seconds,
        trail: false,
    };
    let steps = vec![
        drive(0.0, -100.0, 2.0),
        drive(-100.0, 0.0, 1.5),
        Step {
            kind: StepKind::Explode {
                targets: vec![Target::Component { id: d }],
                offset: [0.0, 0.0, 50.0],
            },
            seconds: 1.0,
            trail: false,
        },
        Step {
            kind: StepKind::Explode {
                targets: vec![Target::Body { root: knob }],
                offset: [0.0, 0.0, 40.0],
            },
            seconds: 1.0,
            trail: false,
        },
    ];
    let op = kmb_interchange::animation::set_steps_op(doc.data(), steps).unwrap();
    doc.apply(kmb_core::Transaction::new("段", vec![op]))
        .unwrap();
    doc
}

/// 112〜114. 干渉: 蝶番を 5 度閉じる側へ回して蓋を箱にめり込ませ、
/// 干渉を調べる → 表と赤い重なり → 一致した面を含める → 行を選んで 1 組だけ。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_interference_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut ws = lidded_box();
    let joint = first_of(&ws, "asm.joint@1");
    let mut data = ws.document().data().feature(joint).unwrap().data.clone();
    data["angle"] = "5 deg".into();
    data["angle_max"] = "10 deg".into();
    ws.set_feature_data(joint, data);
    ws.wait();
    // 人の手順で確かめる文書(実際のアプリで開く)
    std::fs::write(
        format!("{dir}/蓋つきの箱-干渉.json"),
        ws.document().to_json(),
    )
    .unwrap();
    let mut s = Screen::new(ws);
    s.h.state_mut().run_action("asm.interference");
    s.settle();
    s.write(&dir, "112-干渉-表と赤い重なり");
    assert!(s.click_exact("一致した面を含める"));
    s.write(&dir, "113-干渉-一致した面を含める");
    assert!(s.click_exact("すべての干渉を表示"));
    s.h.state_mut().table_select(Some(1));
    s.settle();
    s.write(&dir, "114-干渉-押した行だけ");
}

/// 103〜108. アニメーションの作業空間(24b): 前の設計の作業空間、作業空間の一覧、段の帯の初め・
/// 蓋が開く途中・分解の終わり、動画の見本。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_animation_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let ws = lidded_box();
    std::fs::write(
        format!("{dir}/蓋つきの箱-アニメーション.json"),
        ws.document().to_json(),
    )
    .unwrap();
    let mut s = Screen::new(ws);
    s.write(&dir, "103-前-設計の作業空間(ジョイントのある箱)");
    assert!(s.click("作業空間: 設計"));
    s.write(&dir, "104-作業空間の一覧にアニメーション");
    s.h.state_mut()
        .switch_space(kmb_ui::layout::Space::Animation);
    s.settle();
    s.h.state_mut().set_animation_time(0.0);
    s.settle();
    assert!(s.click("全体表示"));
    s.write(&dir, "105-アニメーションの作業空間-初め");
    s.h.state_mut().set_animation_time(1.0);
    s.settle();
    s.write(&dir, "106-段の帯の途中-蓋が開く");
    s.h.state_mut().set_animation_time(5.5);
    s.settle();
    s.write(&dir, "107-段の帯の終わり-分解");
    s.h.state_mut().change_settings(
        "render.scene",
        &serde_json::json!({"video_width": 640, "video_height": 400, "fps": 15}),
    );
    let out = std::path::Path::new(&dir).join("108-アニメーションの動画.gif");
    s.h.state_mut()
        .run_action_with("render.animation_video", Some(out.clone()));
    s.h.state_mut().wait_task();
    assert!(out.exists(), "{:?}", s.h.state().message());
}

/// 解析の場面の文書: 円柱(直径 20・高さ 30)か箱(40×20×16)を 1 つ置き、研究を足す。
fn study_workspace(
    cylinder: bool,
    type_id: &str,
    data: serde_json::Value,
) -> (Workspace, kmb_core::FeatureId) {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::xy();
    let o = s.fixed_point(0.0, 0.0);
    if cylinder {
        s.circle(o, "20 mm", 10.0);
    } else {
        s.rectangle(o, "40 mm", "20 mm", [0.0, 0.0, 40.0, 20.0]);
    }
    let sk = b.sketch("輪郭", s);
    let body = b.feature(
        "solid.extrude@1",
        if cylinder { "円柱" } else { "箱" },
        serde_json::json!({"profile": sk, "distance": if cylinder { "30 mm" } else { "16 mm" }}),
    );
    let mut doc = b.finish("流れの見本");
    let id = doc.alloc_feature_id();
    let mut data = data;
    data["bodies"] = serde_json::json!([{"kind": "body", "feature": body.0}]);
    doc.apply(kmb_core::Transaction::new(
        "研究",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: type_id.into(),
                name: "研究 1".into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules())
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
    ws.layout_mut().space = Space::Simulation;
    ws.wait();
    ws.select(id);
    (ws, id)
}

/// 見本の部品に静的応力の研究を足す(+X の横の面を固定し、上の面を下へ押す)。
fn static_workspace() -> (Workspace, kmb_core::FeatureId) {
    let mut doc = kmb_samples::reference_document();
    let id = doc.alloc_feature_id();
    doc.apply(kmb_core::Transaction::new(
        "研究",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: "fea.static@1".into(),
                name: "静的応力 1".into(),
                data: serde_json::json!({
                    "material": "アルミ",
                    "fixed": {"kind": "face", "feature": 8, "face": "f8:side:e6"},
                    "loads": [{"faces": {"kind": "face", "feature": 8, "face": "f8:end"}, "kind": "force", "value": "100", "dir": "-z"}],
                    "size": "2 mm",
                }),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules())
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
    ws.layout_mut().space = Space::Simulation;
    ws.wait();
    ws.select(id);
    (ws, id)
}

/// 2D の流れ(円柱、空気 0.1 m/s)と 3D の流れ(箱、空気 1 m/s)の条件。
fn flow2d() -> serde_json::Value {
    serde_json::json!({"plane": "xy", "dir": "+x", "speed": "0.1", "fluid": "air", "size": "1 mm"})
}

fn flow3d() -> serde_json::Value {
    serde_json::json!({"plane": "xz", "dir": "+x", "speed": "1", "fluid": "air", "size": "2 mm"})
}

/// 計算が終わるまで画面を回す。
fn wait_run(s: &mut Screen, id: kmb_core::FeatureId) {
    let t0 = std::time::Instant::now();
    while s
        .h
        .state()
        .run_state(id)
        .is_some_and(|st| st.tone == kmb_core::RunTone::Busy)
    {
        std::thread::sleep(std::time::Duration::from_millis(200));
        s.h.step();
        assert!(t0.elapsed().as_secs() < 1200, "計算が終わらない");
    }
    s.settle();
}

/// 100〜. 解析の研究は「実行」で解き、様子(未実行・計算中・済み・古い)を出す。
/// 流れは時刻のコマの帯、3D は表面の圧力と流線、断面は流体だけを滑らかに塗る。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_study_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    // 静的応力: 実行前 → 実行(済み) → 条件を変える(古い)
    let (ws, id) = static_workspace();
    let mut s = Screen::new(ws);
    s.write(&dir, "100-解析-静的応力-実行前");
    assert!(s.click_exact("▶  実行"));
    wait_run(&mut s, id);
    s.write(&dir, "101-解析-静的応力-済み");
    let mut data =
        s.h.state()
            .document()
            .data()
            .feature(id)
            .unwrap()
            .data
            .clone();
    data["loads"][0]["value"] = serde_json::json!("200");
    s.h.state_mut().set_feature_data(id, data);
    s.h.state_mut().wait();
    s.settle();
    s.write(&dir, "102-解析-静的応力-古い");
    // 2D の流れ(円柱): 断面は流体だけを滑らかに、時刻の帯でコマを送る
    let (ws, id) = study_workspace(true, "flow.section@1", flow2d());
    let mut s = Screen::new(ws);
    assert!(s.click_exact("▶  実行"));
    std::thread::sleep(std::time::Duration::from_millis(3000));
    s.settle();
    s.write(&dir, "103-解析-2Dの流れ-計算中");
    wait_run(&mut s, id);
    s.write(&dir, "104-解析-2Dの流れ-最後のコマ");
    assert!(s.click_exact("最初のコマ"));
    for _ in 0..8 {
        assert!(s.click_exact("次のコマ"));
    }
    s.write(&dir, "105-解析-2Dの流れ-途中のコマ");
    // 3D の流れ(箱): 表面の圧力と流線
    let (ws, id) = study_workspace(false, "flow.volume@1", flow3d());
    let mut s = Screen::new(ws);
    assert!(s.click_exact("▶  実行"));
    wait_run(&mut s, id);
    s.write(&dir, "106-解析-3Dの流れ-表面の圧力と流線");
    assert!(s.click_exact("断面を出す"));
    s.write(&dir, "107-解析-3Dの流れ-断面も出す");
}

/// 108〜110・115〜117. 音の放射・散乱: 解析のツールバーのまとまり「音」から足した研究を「実行」で解き、
/// 指向性(ボディの横の受け取りの球)と表面の音圧レベルを周波数のコマで見る。点音源の断面の音圧レベル(115・116)。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_sound_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    // 箱の上の面を振動させる(1〜4 kHz を 1 オクターブごと)
    let (ws, id) = study_workspace(
        false,
        "wave.radiate@1",
        serde_json::json!({"source": "none", "dir": "+x", "detail": "normal", "f_start": "1000", "f_end": "4000", "per_octave": "1"}),
    );
    let body = ws
        .document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == "solid.extrude@1")
        .unwrap()
        .id
        .0;
    let mut data = ws.document().data().feature(id).unwrap().data.clone();
    data["faces"] =
        serde_json::json!([{"kind": "face", "feature": body, "face": format!("f{body}:end")}]);
    let mut s = Screen::new(ws);
    s.h.state_mut().set_feature_data(id, data);
    s.h.state_mut().wait();
    s.h.state_mut().select(id);
    s.settle();
    s.write(&dir, "108-解析-音の放射・散乱-実行前");
    assert!(s.click_exact("▶  実行") || s.click_exact("実行"));
    wait_run(&mut s, id);
    s.write(&dir, "109-解析-音の放射・散乱-指向性");
    assert!(s.click_exact("最初のコマ"));
    s.write(&dir, "110-解析-音の放射・散乱-最初のコマ");
    // 編集パネルの実行の欄に、周波数ごとの軸の上の音圧レベルのグラフ(対数の横軸)が開いて出る
    s.h.state_mut().open_edit(id);
    s.settle();
    s.write(&dir, "117-解析-音の放射・散乱-周波数のグラフ");

    // 箱の左に点音源を置き、真ん中を通る XY 平面の断面で音圧レベルを見る(8 kHz)
    let (ws, id) = study_workspace(
        false,
        "wave.radiate@1",
        serde_json::json!({"source": "point", "point": [-40.0, 10.0, 8.0], "detail": "normal", "section": "xy", "f_start": "8000", "f_end": "8000"}),
    );
    let mut s = Screen::new(ws);
    s.h.state_mut().wait();
    s.h.state_mut().select(id);
    s.settle();
    s.write(&dir, "115-解析-音の放射・散乱-点音源と断面-実行前");
    assert!(s.click_exact("▶  実行") || s.click_exact("実行"));
    wait_run(&mut s, id);
    assert!(s.click_exact("全体表示"));
    s.write(&dir, "116-解析-音の放射・散乱-点音源と断面");
}

/// 120〜123. 強度の網は形に沿った 2 次の四面体(穴とボスが残り、曲面に沿う)。断面で切ると中の応力、「網の質」で要素の形の良し悪しを色で見る。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_tet_mesh_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let (ws, id) = static_workspace();
    // 人の手順で確かめる文書(実際のアプリで開く)
    std::fs::write(
        format!("{dir}/見本の部品-静的応力.json"),
        ws.document().to_json(),
    )
    .unwrap();
    let mut s = Screen::new(ws);
    assert!(s.click_exact("▶  実行"));
    wait_run(&mut s, id);
    s.write(&dir, "120-解析-四面体の網-静的応力");
    assert!(s.click_exact("断面で切る"));
    s.write(&dir, "121-解析-四面体の網-断面");
    assert!(s.click_exact("断面を隠す"));
    assert!(s.click_exact("網の質"));
    s.write(&dir, "122-解析-四面体の網-網の質");
    assert!(s.click_exact("結果の数値"));
    s.write(&dir, "123-解析-四面体の網-結果の数値");
}

/// 124〜129. ミーゼス応力は塗る点ごとに応力の成分を補間してから求める
/// (断面に格子の模様、手前の面にギザギザが出ない)。固定した縁と入隅に特異点の候補の印。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_stress_paint_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let (ws, id) = static_workspace();
    let mut s = Screen::new(ws);
    assert!(s.click_exact("▶  実行"));
    wait_run(&mut s, id);
    assert!(s.click_exact("断面で切る"));
    s.write(&dir, "124-解析-応力の塗り方-断面-変形あり");
    s.h.state_mut().set_view(-90.0, 90.0);
    s.settle();
    s.write(&dir, "125-解析-応力の塗り方-断面-上から-変形あり");
    assert!(s.click_exact("変形を隠す"));
    s.write(&dir, "126-解析-応力の塗り方-断面-上から-変形なし");
    s.h.state_mut().set_view(-50.0, 30.0);
    s.settle();
    s.write(&dir, "127-解析-応力の塗り方-断面-変形なし");
    assert!(s.click_exact("断面を隠す"));
    s.write(&dir, "128-解析-応力の塗り方-表面と特異点の印");
    assert!(s.click_exact("結果の数値"));
    s.write(&dir, "129-解析-応力の塗り方-結果の数値");
}

/// 109〜111. 部品表: 表の窓、列の設定を開いて「種類」と「合計質量」を出した所。
/// 蓋つきの箱に材料を付ける(本体と蓋は木材、つまみは真鍮)。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_bom_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut ws = lidded_box();
    let roots: Vec<_> = ws.body_roots().iter().map(|r| r.1).collect();
    for (r, m) in roots.iter().zip(["木材", "木材", "真鍮"]) {
        assert!(ws.set_body_material(*r, m));
    }
    ws.wait();
    // 人の手順で確かめる文書(実際のアプリで開く)
    std::fs::write(
        format!("{dir}/蓋つきの箱-部品表.json"),
        ws.document().to_json(),
    )
    .unwrap();
    let mut s = Screen::new(ws);
    s.h.state_mut().run_action("asm.bom");
    s.settle();
    s.write(&dir, "109-部品表の表の窓");
    assert!(s.click_exact("列…"));
    s.write(&dir, "110-部品表の列の設定");
    // 「種類」は画面のほかの所にもあるので、押す代わりに同じ操作を呼ぶ(押す流れは kmb-ui の試験)
    s.h.state_mut().table_column("kind", 0);
    s.h.state_mut().table_column("total_mass", 0);
    s.settle();
    s.write(&dir, "111-部品表に種類と合計質量を出した");
}

/// 映り込みの見本(環境マップ): 暗い画面と明るい画面で、斜めからと逆側から。
/// アプリで開く文書(映り込みの見本.json)も書く。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_look_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        format!("{dir}/映り込みの見本.json"),
        kmb_samples::look_document().to_json(),
    )
    .unwrap();
    use kmb_ui::layout::Brightness;
    for (theme, b) in [("暗い", Brightness::Dark), ("明るい", Brightness::Light)] {
        for (name, yaw, pitch) in [("斜め", -50.0, 35.0), ("逆側", 130.0, 35.0)] {
            let mut w = Workspace::new(kmb_samples::look_document(), None, kmb_bundle::modules())
                .with_editor(kmb_ui_sketch::SketchEditor::new())
                .with_editor(kmb_ui_tools::ToolsEditor::new());
            w.layout_mut().brightness = b;
            let mut s = Screen::new(w);
            s.h.state_mut().clear_selection();
            s.h.state_mut().set_view(yaw, pitch);
            s.settle();
            s.write(&dir, &format!("映り込み-{theme}-{name}"));
        }
    }
}

/// 下の帯(タイムラインの部品)と 3D ビューの境を、人と同じく引いて `by` だけ上へ広げる。
/// 境の所はマウスを置くと上下の矢印に変わるので、ビューの下の帯とタイムラインの間を 1 点ずつ探す。
fn widen_timeline(s: &mut Screen, by: f32) {
    let below = s.h.get_by_label("ロールバックの棒を先頭へ").rect().top();
    let mut at = None;
    for k in 0..200 {
        let y = below - k as f32;
        let p = egui::pos2(s.size[0] * 0.5, y);
        s.h.event(egui::Event::PointerMoved(p));
        s.h.step();
        if s.h.output().platform_output.cursor_icon == egui::CursorIcon::ResizeVertical {
            at = Some(p);
            break;
        }
    }
    let from = at.expect("タイムラインの上の境が見つからない");
    let to = from - egui::vec2(0.0, by);
    s.h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    s.h.step();
    for k in 1..=12 {
        s.h.event(egui::Event::PointerMoved(
            from + (to - from) * (k as f32 / 12.0),
        ));
        s.h.step();
    }
    s.h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    s.settle();
}

/// 設計のタイムライン: 既定の高さ(アイコンだけ)と、境を引いて広げた時(道具と
/// グループの名前)を、幅 1280・字とアイコン 100 %・250 % で撮る。`JC_SCREENS_OUT` に書く。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_timeline_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    for (pct, k) in [(100, 1.0), (250, 2.5)] {
        // 2 つ目と 3 つ目、5 つ目から 7 つ目をグループにする(入れ子にしない 2 つ)
        let mut doc = kmb_samples::reference_document();
        let reg = kmb_core::Registry::new();
        for (name, a, b) in [("板", 1, 2), ("穴", 4, 6)] {
            let ids: Vec<_> = doc.data().features[a..=b].iter().map(|f| f.id).collect();
            let tx = kmb_session::edit::make_group(
                &mut doc,
                &reg,
                &ids,
                name,
                kmb_session::edit::GroupMode::Exact,
            )
            .unwrap();
            doc.apply(tx).unwrap();
        }
        let mut w = Workspace::new(doc, None, kmb_bundle::modules())
            .with_editor(kmb_ui_sketch::SketchEditor::new())
            .with_editor(kmb_ui_tools::ToolsEditor::new());
        w.layout_mut().appearance.text_scale = k;
        w.layout_mut().appearance.icon_scale = k;
        let mut s = Screen::sized(w, Some([1280.0, 800.0]));
        s.write(&dir, &format!("タイムライン-{pct}-既定"));
        widen_timeline(&mut s, 40.0 * k);
        s.write(&dir, &format!("タイムライン-{pct}-広げた"));
    }
}

/// スケッチ: 単独の点を 3D のビューに出す(t9)と、円弧の端より
/// 外に置いた半径の寸法で円弧を点線で延ばす(t4)を撮る。`JC_SCREENS_OUT` に書く。
#[test]
#[ignore = "道具: JC_SCREENS_OUT に画面の写真を書く(目で見る確かめ)"]
fn write_sketch_point_and_arc_screens() {
    let Ok(dir) = std::env::var("JC_SCREENS_OUT") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let doc = kmb_samples::sample::arc_and_point();
    let id = doc.data().features[0].id;
    let w = Workspace::new(doc, None, kmb_bundle::modules())
        .with_editor(kmb_ui_sketch::SketchEditor::new())
        .with_editor(kmb_ui_tools::ToolsEditor::new());
    let mut s = Screen::sized(w, Some([1280.0, 800.0]));
    s.write(&dir, "スケッチの単独の点-3Dのビュー");
    s.h.state_mut().open_edit(id);
    s.settle();
    s.write(&dir, "円弧の端より外の半径の寸法");
}
