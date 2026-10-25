//! 工具ライブラリの読み込みと書き出し(ほかの CAM との受け渡し)。
//!
//! 読めるもの:
//! - Fusion の工具ライブラリの JSON(`{"data": [...], "version": n}`)と、それを zip にした `.tools`
//! - この CAD・CAM の工具ライブラリ(`{"tools": [...]}`、PC 全体のライブラリの `tools.json` と同じ形)
//!
//! 書けるもの: Fusion の形の JSON と `.tools`、表計算で見る CSV(Fusion の CSV の列名のうち使う列だけ)。
//!
//! 形は版 36 の `.tools` で確かめた(taperAngle は片側の角度)。テーパーのエンドミル(`tapered mill`)の
//! 径を先の径とみなす所は推測を含む。
//!
//! 読めない種類(ねじ切り、レーザー、ホルダーなど)は飛ばし、種類ごとの本数を返す。ねじ切りミルは
//! 書き出せるが、歯の数とピッチの項目名を Fusion の文書で確かめていないので、形だけを書き、読む時は飛ばす。

use crate::{Cutting, Extra, Preset, Tool, ToolKind, ToolLibrary, material};
use serde_json::{Map, Value as Json, json};

/// 読み込んだ結果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Imported {
    /// 読めた工具(番号はファイルのまま。重なりは足す時に振り直す)
    pub tools: Vec<Tool>,
    /// 飛ばした工具(理由 → 本数)。理由は種類の名前か、寸法の誤り
    pub skipped: Vec<(String, usize)>,
}

impl Imported {
    fn skip(&mut self, why: String) {
        match self.skipped.iter_mut().find(|(w, _)| *w == why) {
            Some((_, n)) => *n += 1,
            None => self.skipped.push((why, 1)),
        }
    }

    /// 飛ばした本数。
    pub fn skipped_count(&self) -> usize {
        self.skipped.iter().map(|(_, n)| n).sum()
    }

    /// 飛ばした工具の説明(例: 「thread mill 4 本、laser cutter 2 本」)。なければ空。
    pub fn skipped_text(&self) -> String {
        self.skipped
            .iter()
            .map(|(w, n)| format!("{w} {n} 本"))
            .collect::<Vec<_>>()
            .join("、")
    }
}

/// zip の中のファイルの大きさの上限(壊れたファイルや悪意のあるファイルで固まらないように)。
const MAX_SIZE: usize = 64 << 20;

/// ファイルの中身から工具を読む(zip か JSON かは中身で見分ける)。
pub fn read_library(bytes: &[u8]) -> Result<Imported, String> {
    let owned;
    let text = if bytes.starts_with(b"PK\x03\x04") {
        owned = unzip_json(bytes)?;
        &owned[..]
    } else {
        bytes
    };
    let text = std::str::from_utf8(text).map_err(|_| "文字コードが UTF-8 ではありません")?;
    let v: Json = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("JSON として読めません: {e}"))?;
    if v.get("data").is_some_and(Json::is_array) {
        return Ok(from_fusion(&v));
    }
    if v.get("tools").is_some_and(Json::is_array) {
        let lib: ToolLibrary = serde_json::from_value(v)
            .map_err(|e| format!("工具ライブラリとして読めません: {e}"))?;
        let mut out = Imported::default();
        for t in lib.tools {
            match t.check() {
                Ok(()) => out.tools.push(t),
                Err(e) => out.skip(e),
            }
        }
        return Ok(out);
    }
    Err("工具ライブラリではありません(ほかの CAM の工具ライブラリ(.tools・.json)か、この CAM の工具の一覧を選んでください)".into())
}

/// ファイルの拡張子(点なし、小文字)に合う形で書く。`csv`・`tools`・それ以外は JSON。
pub fn write_library(lib: &ToolLibrary, extension: &str) -> Vec<u8> {
    match extension.to_ascii_lowercase().as_str() {
        "csv" => to_csv(lib).into_bytes(),
        "tools" => zip_one(
            "tools.json",
            serde_json::to_string_pretty(&to_fusion(lib))
                .expect("JSON にできる")
                .as_bytes(),
        ),
        _ => serde_json::to_string_pretty(&to_fusion(lib))
            .expect("JSON にできる")
            .into_bytes(),
    }
}

// ---- Fusion の JSON から ----

fn num(o: &Json, key: &str) -> Option<f64> {
    o.get(key).and_then(Json::as_f64).filter(|v| v.is_finite())
}

/// Fusion の工具の種類と、この CAM の種類。
fn kind_of(fusion_type: &str, g: &Json) -> Option<ToolKind> {
    let re = num(g, "RE").unwrap_or(0.0);
    let dc = num(g, "DC").unwrap_or(0.0);
    Some(match fusion_type {
        "flat end mill" if re > 1e-9 => ToolKind::BullNose,
        "flat end mill" => ToolKind::FlatEnd,
        "ball end mill" => ToolKind::BallEnd,
        "bull nose end mill" if re <= 1e-9 => ToolKind::FlatEnd,
        "bull nose end mill" if re >= dc / 2.0 - 1e-9 => ToolKind::BallEnd,
        "bull nose end mill" => ToolKind::BullNose,
        "drill" | "spot drill" => ToolKind::Drill,
        "chamfer mill" if num(g, "tip-diameter").unwrap_or(0.0) > 1e-9 => ToolKind::TaperEnd,
        "chamfer mill" => ToolKind::VBit,
        "tapered mill" if re > 1e-9 => ToolKind::TaperBall,
        "tapered mill" => ToolKind::TaperEnd,
        _ => return None,
    })
}

/// Fusion の工具表を読む。
pub fn from_fusion(v: &Json) -> Imported {
    let mut out = Imported::default();
    for t in v["data"].as_array().into_iter().flatten() {
        let ty = t["type"].as_str().unwrap_or("").to_string();
        let g = &t["geometry"];
        let Some(kind) = kind_of(&ty, g) else {
            out.skip(if ty.is_empty() {
                "種類のない工具".into()
            } else {
                ty
            });
            continue;
        };
        match fusion_tool(t, kind) {
            Ok(tool) => out.tools.push(tool),
            Err(e) => out.skip(e),
        }
    }
    out
}

fn fusion_tool(t: &Json, kind: ToolKind) -> Result<Tool, String> {
    let g = &t["geometry"];
    // 長さの単位(インチなら mm にする)。角度と回転数は単位によらない
    let k = match t["unit"].as_str() {
        Some("inches") => 25.4,
        _ => 1.0,
    };
    let len = |key: &str| num(g, key).map(|v| v * k).filter(|v| *v > 0.0);
    let dc = len("DC").ok_or("径のない工具")?;
    let lcf = len("LCF").ok_or("刃長のない工具")?;
    let half = num(g, "TA").unwrap_or(0.0);
    let re = len("RE").unwrap_or(0.0);
    let mut tool = Tool {
        number: t["post-process"]["number"].as_u64().unwrap_or(0) as u32,
        name: String::new(),
        kind,
        diameter: dc,
        flute_length: lcf,
        flutes: num(g, "NOF").map_or(2, |n| n.round().max(1.0) as u32),
        tip_angle: 118.0,
        shank_diameter: len("SFDM"),
        overall_length: len("OAL").filter(|l| *l >= lcf),
        tip_diameter: None,
        stickout: None,
        holder_diameter: None,
        corner_radius: None,
        neck_length: len("shoulder-length").filter(|n| *n > lcf + 1e-6),
        thread: None,
        presets: Vec::new(),
        extra: Extra::new(),
    };
    match kind {
        ToolKind::Drill => tool.tip_angle = num(g, "SIG").unwrap_or(118.0),
        ToolKind::VBit => tool.tip_angle = 2.0 * half,
        ToolKind::BullNose => tool.corner_radius = Some(re.min(dc / 2.0)),
        ToolKind::TaperEnd | ToolKind::TaperBall => {
            tool.tip_angle = 2.0 * half;
            if t["type"] == "tapered mill" {
                // テーパーのエンドミルの径は先の径。刃の上端の径は刃長とテーパーの角度から求め、
                // シャンクより太くはしない
                let tip = if kind == ToolKind::TaperBall {
                    2.0 * re
                } else {
                    dc
                };
                let mut top = tip + 2.0 * lcf * half.to_radians().tan();
                if let Some(s) = tool.shank_diameter.filter(|s| *s >= tip) {
                    top = top.min(s);
                }
                tool.diameter = top.max(tip);
                tool.tip_diameter = Some(tip);
            } else {
                tool.tip_diameter = len("tip-diameter").map(|d| d.min(dc));
            }
        }
        ToolKind::FlatEnd | ToolKind::BallEnd | ToolKind::Laser | ToolKind::ThreadMill => {}
    }
    // ホルダーの下の長さ(突き出し)と、ホルダーのいちばん太い所の径
    if let Some(segs) = t["holder"]["segments"].as_array() {
        let hd = segs
            .iter()
            .flat_map(|s| [num(s, "lower-diameter"), num(s, "upper-diameter")])
            .flatten()
            .fold(0.0, f64::max)
            * holder_unit(t, k);
        if hd > 0.0 {
            tool.holder_diameter = Some(hd);
            tool.stickout = len("LB").filter(|l| *l >= lcf);
        }
    }
    tool.name = match t["description"].as_str().map(str::trim) {
        Some(d) if !d.is_empty() => d.to_string(),
        _ => format!("{} {} mm", kind.label(), round(tool.diameter)),
    };
    for (key, src) in [
        ("fusion_guid", &t["guid"]),
        ("vendor", &t["vendor"]),
        ("product_link", &t["product-link"]),
        ("tool_material", &t["BMC"]),
    ] {
        if let Some(s) = src.as_str().filter(|s| !s.is_empty()) {
            tool.extra.insert(key.into(), s.into());
        }
    }
    tool.presets = presets_of(t, &tool, k);
    tool.check()?;
    Ok(tool)
}

/// ホルダーの長さの単位(ホルダーは工具と別に単位を持つ)。
fn holder_unit(t: &Json, tool_k: f64) -> f64 {
    match t["holder"]["unit"].as_str() {
        Some("inches") => 25.4,
        Some("millimeters") => 1.0,
        _ => tool_k,
    }
}

fn presets_of(t: &Json, tool: &Tool, k: f64) -> Vec<Preset> {
    let def = Cutting::default();
    let default_stepover = if tool.kind == ToolKind::BallEnd {
        0.15
    } else {
        0.4
    };
    let mut out: Vec<Preset> = Vec::new();
    for p in t["start-values"]["presets"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let on = |key: &str| p.get(key).and_then(Json::as_bool).unwrap_or(true);
        let stepdown = num(p, "stepdown")
            .filter(|_| on("use-stepdown"))
            .map(|v| v * k)
            .filter(|v| *v > 0.0)
            .unwrap_or_else(|| (tool.diameter * 0.3).clamp(0.05, 1.0));
        let stepover = num(p, "stepover")
            .filter(|_| on("use-stepover"))
            .map(|v| v * k / tool.diameter)
            .filter(|v| *v > 0.0)
            .unwrap_or(default_stepover)
            .clamp(1e-4, 1.0);
        // 回転数と送りは 0.001 に丸める(Fusion は周速から求めた端数を持つ)
        let positive = |key: &str, f: f64, d: f64| {
            num(p, key)
                .map(|v| (v * f * 1000.0).round() / 1000.0)
                .filter(|v| *v > 0.0)
                .unwrap_or(d)
        };
        let cutting = Cutting {
            rpm: positive("n", 1.0, def.rpm),
            feed: positive("v_f", k, def.feed),
            plunge: positive("v_f_plunge", k, def.plunge),
            stepdown,
            stepover,
        };
        let raw = p["name"].as_str().unwrap_or("").trim();
        // 知っている材料は、この CAM の名前にそろえる(同じ材料が 2 つ目なら元の名前のまま)
        let name = match raw {
            "" | "既定" => String::new(),
            _ => match material(raw) {
                Some(m) if !out.iter().any(|q| q.material == m.name) => m.name.to_string(),
                _ => raw.to_string(),
            },
        };
        out.push(Preset {
            material: name,
            cutting,
        });
    }
    if out.is_empty() {
        out.push(Preset {
            material: String::new(),
            cutting: Cutting {
                stepover: default_stepover,
                stepdown: (tool.diameter * 0.3).clamp(0.05, 1.0),
                ..def
            },
        });
    }
    out
}

fn round(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

// ---- Fusion の JSON へ ----

/// 工具ごとに決まる GUID の形の文字列(同じ工具を書き出し直しても同じ値にする)。
fn guid_for(t: &Tool, salt: &str) -> String {
    if salt.is_empty()
        && let Some(g) = t.extra.get("fusion_guid").and_then(Json::as_str)
    {
        return g.to_string();
    }
    // FNV-1a を 2 回(64 ビットを 2 つ)
    let hash = |seed: u64| {
        let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
        for b in format!("{}|{}|{salt}", t.number, t.name).bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    let (a, b) = (hash(1), hash(2));
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0x0fff,
        0x8000 | ((b >> 48) & 0x3fff),
        b & 0xffff_ffff_ffff
    )
}

fn fusion_type(k: ToolKind) -> &'static str {
    match k {
        ToolKind::FlatEnd => "flat end mill",
        ToolKind::BallEnd => "ball end mill",
        ToolKind::BullNose => "bull nose end mill",
        ToolKind::Drill => "drill",
        ToolKind::VBit | ToolKind::TaperEnd => "chamfer mill",
        ToolKind::TaperBall => "tapered mill",
        ToolKind::Laser => "laser cutter",
        ToolKind::ThreadMill => "thread mill",
    }
}

/// Fusion の形の 1 本。
fn fusion_tool_json(t: &Tool) -> Json {
    let mut g = Map::new();
    let tip_d = t.tip_diameter.unwrap_or(0.0);
    let dc = if t.kind == ToolKind::TaperBall {
        tip_d
    } else {
        t.diameter
    };
    g.insert("CSP".into(), false.into());
    g.insert("HAND".into(), true.into());
    g.insert("DC".into(), dc.into());
    g.insert("LCF".into(), t.flute_length.into());
    g.insert("NOF".into(), t.flutes.into());
    g.insert("SFDM".into(), t.shank().into());
    g.insert(
        "shoulder-length".into(),
        t.neck_length.unwrap_or(t.flute_length).into(),
    );
    if let Some(l) = t.overall_length {
        g.insert("OAL".into(), l.into());
    }
    if let Some(s) = t.stickout {
        g.insert("LB".into(), s.into());
    }
    match t.kind {
        ToolKind::BallEnd => {
            g.insert("RE".into(), t.radius().into());
        }
        ToolKind::BullNose => {
            g.insert("RE".into(), t.corner_radius.unwrap_or(0.0).into());
        }
        ToolKind::Drill => {
            g.insert("SIG".into(), t.tip_angle.into());
        }
        ToolKind::VBit | ToolKind::TaperEnd => {
            g.insert("TA".into(), (t.tip_angle / 2.0).into());
            g.insert("tip-diameter".into(), tip_d.into());
        }
        ToolKind::TaperBall => {
            g.insert("TA".into(), (t.tip_angle / 2.0).into());
            g.insert("RE".into(), (tip_d / 2.0).into());
            g.insert("tip-diameter".into(), tip_d.into());
        }
        // ねじ切りミルの歯の数とピッチの項目名は Fusion の文書で確かめられていないので書かない
        // (読む側も飛ばす。下の `kind_of`)
        ToolKind::FlatEnd | ToolKind::Laser | ToolKind::ThreadMill => {}
    }
    let presets: Vec<Json> = t
        .presets
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let c = &p.cutting;
            let name = if p.material.is_empty() {
                "既定".to_string()
            } else {
                p.material.clone()
            };
            json!({
                "guid": guid_for(t, &format!("preset{i}")),
                "name": name,
                "n": c.rpm,
                "n_ramp": c.rpm,
                "v_f": c.feed,
                "v_f_plunge": c.plunge,
                "v_f_ramp": c.plunge,
                "v_f_leadIn": c.feed,
                "v_f_leadOut": c.feed,
                "v_f_transition": c.feed,
                "f_z": c.chip_load(t.flutes),
                "f_n": c.feed / c.rpm,
                "v_c": std::f64::consts::PI * t.diameter * c.rpm / 1000.0,
                "stepdown": c.stepdown,
                "stepover": c.stepover * t.diameter,
                "use-stepdown": true,
                "use-stepover": true,
                "tool-coolant": "disabled",
                "material": {"category": "all", "query": "", "use-hardness": false},
            })
        })
        .collect();
    let s = |key: &str| t.extra.get(key).and_then(Json::as_str).unwrap_or("");
    let mut o = json!({
        "guid": guid_for(t, ""),
        "type": fusion_type(t.kind),
        "unit": "millimeters",
        "description": t.name,
        "vendor": s("vendor"),
        "product-id": "",
        "product-link": s("product_link"),
        "BMC": if s("tool_material").is_empty() { "carbide" } else { s("tool_material") },
        "geometry": Json::Object(g),
        "post-process": {
            "number": t.number,
            "break-control": false,
            "comment": "",
            "diameter-offset": t.number,
            "length-offset": t.number,
            "live": true,
            "manual-tool-change": false,
            "turret": 0,
        },
        "start-values": {"presets": presets},
    });
    if let Some(hd) = t.holder_diameter {
        o["holder"] = json!({
            "guid": guid_for(t, "holder"),
            "type": "holder",
            "unit": "millimeters",
            "description": format!("ホルダー {} mm", round(hd)),
            "segments": [{"height": hd, "lower-diameter": hd, "upper-diameter": hd}],
        });
    }
    o
}

/// Fusion の工具ライブラリの JSON にする。
pub fn to_fusion(lib: &ToolLibrary) -> Json {
    json!({
        "data": lib.tools.iter().map(fusion_tool_json).collect::<Vec<_>>(),
        "version": 36,
    })
}

// ---- CSV ----

/// CSV にする(1 行が 1 本の工具の 1 つの材料。列名は Fusion の CSV の列名にそろえる)。
pub fn to_csv(lib: &ToolLibrary) -> String {
    const HEAD: &[&str] = &[
        "Number (tool_number)",
        "Description (tool_description)",
        "Type (tool_type)",
        "Unit (tool_unit)",
        "Preset Name (preset_name)",
        "Diameter (tool_diameter)",
        "Flute Length (tool_fluteLength)",
        "Overall Length (tool_overallLength)",
        "Shoulder Length (tool_shoulderLength)",
        "Body Length (tool_bodyLength)",
        "Shaft Diameter (tool_shaftDiameter)",
        "Number of Flutes (tool_numberOfFlutes)",
        "Corner Radius (tool_cornerRadius)",
        "Taper Angle (tool_taperAngle)",
        "Tip Angle (tool_tipAngle)",
        "Tip Diameter (tool_tipDiameter)",
        "Spindle Speed (tool_spindleSpeed)",
        "Cutting Feedrate (tool_feedCutting)",
        "Plunge Feedrate (tool_feedPlunge)",
        "Feed per Tooth (tool_feedPerTooth)",
        "Stepdown (tool_stepdown)",
        "Stepover (tool_stepover)",
    ];
    let field = |s: String| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s
        }
    };
    let opt = |v: Option<f64>| v.map(|v| round(v).to_string()).unwrap_or_default();
    let mut out = HEAD
        .iter()
        .map(|h| field((*h).to_string()))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    for t in &lib.tools {
        let (taper, tip_angle) = match t.kind {
            ToolKind::VBit | ToolKind::TaperEnd | ToolKind::TaperBall => {
                (Some(t.tip_angle / 2.0), None)
            }
            ToolKind::Drill => (None, Some(t.tip_angle)),
            _ => (None, None),
        };
        let corner = match t.kind {
            ToolKind::BallEnd => Some(t.radius()),
            ToolKind::BullNose => t.corner_radius,
            ToolKind::TaperBall => t.tip_diameter.map(|d| d / 2.0),
            _ => None,
        };
        for p in &t.presets {
            let c = &p.cutting;
            let row = [
                t.number.to_string(),
                t.name.clone(),
                fusion_type(t.kind).into(),
                "millimeters".into(),
                p.material.clone(),
                round(t.diameter).to_string(),
                round(t.flute_length).to_string(),
                opt(t.overall_length),
                opt(t.neck_length),
                opt(t.stickout),
                round(t.shank()).to_string(),
                t.flutes.to_string(),
                opt(corner),
                opt(taper),
                opt(tip_angle),
                opt(t.tip_diameter),
                round(c.rpm).to_string(),
                round(c.feed).to_string(),
                round(c.plunge).to_string(),
                round(c.chip_load(t.flutes)).to_string(),
                round(c.stepdown).to_string(),
                round(c.stepover * t.diameter).to_string(),
            ];
            out.push_str(&row.map(field).join(","));
            out.push('\n');
        }
    }
    out
}

// ---- zip ----

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// zip の中の最初の `.json` を取り出す(`.tools` は `tools.json` 1 つの zip)。
pub fn unzip_json(b: &[u8]) -> Result<Vec<u8>, String> {
    const BAD: &str = "zip として読めません(.tools のファイルが壊れています)";
    // 終わりの記録(後ろから探す。コメントは最長 65535 バイト)
    let from = b.len().saturating_sub(22 + 65535);
    let end = (from..=b.len().saturating_sub(22))
        .rev()
        .find(|&i| u32_at(b, i) == Some(0x0605_4b50))
        .ok_or(BAD)?;
    let count = u16_at(b, end + 10).ok_or(BAD)? as usize;
    let mut at = u32_at(b, end + 16).ok_or(BAD)? as usize;
    for _ in 0..count {
        if u32_at(b, at) != Some(0x0201_4b50) {
            return Err(BAD.into());
        }
        let method = u16_at(b, at + 10).ok_or(BAD)?;
        let crc = u32_at(b, at + 16).ok_or(BAD)?;
        let csize = u32_at(b, at + 20).ok_or(BAD)? as usize;
        let usize_ = u32_at(b, at + 24).ok_or(BAD)? as usize;
        let n = u16_at(b, at + 28).ok_or(BAD)? as usize;
        let e = u16_at(b, at + 30).ok_or(BAD)? as usize;
        let c = u16_at(b, at + 32).ok_or(BAD)? as usize;
        let local = u32_at(b, at + 42).ok_or(BAD)? as usize;
        let name = String::from_utf8_lossy(b.get(at + 46..at + 46 + n).ok_or(BAD)?).to_lowercase();
        at += 46 + n + e + c;
        if !name.ends_with(".json") {
            continue;
        }
        if u32_at(b, local) != Some(0x0403_4b50) || usize_ > MAX_SIZE {
            return Err(BAD.into());
        }
        let ln = u16_at(b, local + 26).ok_or(BAD)? as usize;
        let le = u16_at(b, local + 28).ok_or(BAD)? as usize;
        let start = local + 30 + ln + le;
        let data = b.get(start..start + csize).ok_or(BAD)?;
        let out = match method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, MAX_SIZE)
                .map_err(|_| BAD)?,
            m => return Err(format!("zip の圧縮の方式 {m} は読めません")),
        };
        if crc32fast::hash(&out) != crc {
            return Err(BAD.into());
        }
        return Ok(out);
    }
    Err("zip の中に工具の一覧(.json)がありません".into())
}

/// 1 つのファイルだけを入れた zip を作る(圧縮は deflate)。
pub fn zip_one(name: &str, data: &[u8]) -> Vec<u8> {
    jc_format::zip::write(&[(name, data)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fusion の工具表の見本(値はこの試験のために作った)。
    fn sample() -> Json {
        json!({"version": 36, "data": [
            {"type": "flat end mill", "unit": "millimeters", "description": "平 3.175×12",
             "guid": "g-1", "vendor": "V", "BMC": "carbide",
             "geometry": {"DC": 3.175, "LCF": 12, "NOF": 1, "OAL": 38, "SFDM": 3.175, "LB": 19,
                          "shoulder-length": 12},
             "holder": {"unit": "millimeters", "segments": [
                 {"height": 6.5, "lower-diameter": 7.5, "upper-diameter": 7.5},
                 {"height": 0.5, "lower-diameter": 9, "upper-diameter": 10}]},
             "post-process": {"number": 1},
             "start-values": {"presets": [
                 {"name": "Aluminum", "n": 12000, "v_f": 500, "v_f_plunge": 200, "stepdown": 0.2,
                  "use-stepdown": true, "use-stepover": false},
                 {"name": "Hardwood", "n": 12000, "v_f": 1000, "v_f_plunge": 300, "stepdown": 1,
                  "stepover": 1.27, "use-stepdown": true, "use-stepover": true},
                 {"name": "Softwood", "n": 12000, "v_f": 1000, "v_f_plunge": 300, "stepdown": 2,
                  "use-stepdown": true, "use-stepover": false}]}},
            {"type": "chamfer mill", "unit": "millimeters", "description": "彫刻 30° 0.2",
             "geometry": {"DC": 3.175, "LCF": 9, "NOF": 1, "TA": 15, "tip-diameter": 0.2},
             "post-process": {"number": 1}},
            {"type": "chamfer mill", "unit": "millimeters", "description": "",
             "geometry": {"DC": 6, "LCF": 5, "NOF": 2, "TA": 45},
             "post-process": {"number": 3}},
            {"type": "bull nose end mill", "unit": "inches", "description": "Bull 1/4",
             "geometry": {"DC": 0.25, "LCF": 0.75, "NOF": 3, "RE": 0.03, "OAL": 2.5},
             "post-process": {"number": 4},
             "start-values": {"presets": [
                 {"name": "Aluminum", "n": 10000, "v_f": 40, "v_f_plunge": 10, "stepdown": 0.05,
                  "stepover": 0.1}]}},
            {"type": "tapered mill", "unit": "millimeters", "description": "テーパーボール R0.25",
             "geometry": {"DC": 0.5, "LCF": 20, "NOF": 2, "TA": 3, "RE": 0.25, "SFDM": 3.175},
             "post-process": {"number": 5}},
            {"type": "drill", "unit": "millimeters", "description": "ドリル 1",
             "geometry": {"DC": 1, "LCF": 10, "SIG": 118}, "post-process": {"number": 6}},
            {"type": "thread mill", "unit": "millimeters", "description": "M3",
             "geometry": {"DC": 2.42, "LCF": 2}, "post-process": {"number": 7}},
            {"type": "thread mill", "unit": "millimeters", "description": "M2",
             "geometry": {"DC": 1.54, "LCF": 2}, "post-process": {"number": 8}},
            {"type": "laser cutter", "unit": "millimeters", "description": "laser",
             "geometry": {"CW": 0.1}, "post-process": {"number": 0}},
            {"type": "flat end mill", "unit": "millimeters", "description": "壊れた",
             "geometry": {"DC": 3, "LCF": 0}, "post-process": {"number": 9}}
        ]})
    }

    #[test]
    fn fusion_libraries_are_read_with_units_kinds_and_presets() {
        let bytes = serde_json::to_vec(&sample()).unwrap();
        let im = read_library(&bytes).unwrap();
        assert_eq!(im.tools.len(), 6);
        assert_eq!(im.skipped_count(), 4);
        assert_eq!(
            im.skipped_text(),
            "thread mill 2 本、laser cutter 1 本、刃長のない工具 1 本"
        );
        let t = &im.tools[0];
        assert_eq!((t.kind, t.number, t.flutes), (ToolKind::FlatEnd, 1, 1));
        assert_eq!((t.overall_length, t.stickout), (Some(38.0), Some(19.0)));
        assert_eq!(t.holder_diameter, Some(10.0));
        assert_eq!(t.neck_length, None);
        assert_eq!(t.extra["fusion_guid"], "g-1");
        // 材料の名前をそろえる。送り幅がなければ 0.4、あれば径との比
        let m: Vec<&str> = t.presets.iter().map(|p| p.material.as_str()).collect();
        assert_eq!(m, ["アルミ", "木材", "Softwood"]);
        assert_eq!(t.presets[0].cutting.stepover, 0.4);
        assert!((t.presets[1].cutting.stepover - 0.4).abs() < 0.001);
        assert_eq!(t.presets[0].cutting.feed, 500.0);
        // 彫刻の刃: 片側 15° を全角 30° に、先の径があればテーパー
        let e = &im.tools[1];
        assert_eq!(
            (e.kind, e.tip_angle, e.tip_diameter),
            (ToolKind::TaperEnd, 30.0, Some(0.2))
        );
        let v = &im.tools[2];
        assert_eq!((v.kind, v.tip_angle), (ToolKind::VBit, 90.0));
        assert_eq!(v.name, "V ビット 6 mm");
        // インチを mm に
        let b = &im.tools[3];
        assert_eq!(b.kind, ToolKind::BullNose);
        assert!((b.diameter - 6.35).abs() < 1e-9);
        assert!((b.corner_radius.unwrap() - 0.762).abs() < 1e-9);
        assert!((b.presets[0].cutting.feed - 1016.0).abs() < 1e-9);
        assert!((b.presets[0].cutting.stepover - 0.4).abs() < 1e-9);
        // テーパーボール: 径は先の球の径、上端は刃長と角度から(シャンク径まで)
        let tb = &im.tools[4];
        assert_eq!(tb.kind, ToolKind::TaperBall);
        assert_eq!(tb.tip_diameter, Some(0.5));
        let top = 0.5 + 2.0 * 20.0 * 3f64.to_radians().tan();
        assert!((tb.diameter - top).abs() < 1e-9);
        assert_eq!(
            (im.tools[5].kind, im.tools[5].tip_angle),
            (ToolKind::Drill, 118.0)
        );
    }

    #[test]
    fn written_libraries_read_back_the_same_tools() {
        let mut lib = ToolLibrary::starter();
        lib.add_tools(crate::carvera_air_tools(), true);
        let mut bull = lib.tools[0].clone();
        bull.number = 40;
        bull.name = "ブルノーズ 6 mm R1".into();
        bull.kind = ToolKind::BullNose;
        bull.corner_radius = Some(1.0);
        bull.stickout = Some(25.0);
        bull.holder_diameter = Some(16.0);
        bull.overall_length = Some(50.0);
        let mut tb = lib.tools[0].clone();
        tb.number = 41;
        tb.name = "テーパーボール".into();
        tb.kind = ToolKind::TaperBall;
        tb.tip_diameter = Some(1.0);
        tb.tip_angle = 10.0;
        tb.flute_length = 20.0;
        // 上端の径は刃長と角度から決まる値にしておく(Fusion は上端の径を持たない)
        tb.diameter = 1.0 + 2.0 * 20.0 * 5f64.to_radians().tan();
        tb.shank_diameter = Some(6.0);
        lib.tools.push(bull);
        lib.tools.push(tb);
        lib.check().unwrap();
        for ext in ["json", "tools"] {
            let bytes = write_library(&lib, ext);
            let im = read_library(&bytes).unwrap();
            assert_eq!(im.skipped_count(), 0, "{ext}");
            assert_eq!(im.tools.len(), lib.tools.len());
            for (a, b) in lib.tools.iter().zip(&im.tools) {
                assert_eq!((a.number, &a.name, a.kind), (b.number, &b.name, b.kind));
                assert!((a.diameter - b.diameter).abs() < 1e-9, "{}", a.name);
                assert_eq!(a.flute_length, b.flute_length);
                assert!((a.tip_angle - b.tip_angle).abs() < 1e-9 || a.kind == ToolKind::FlatEnd);
                assert_eq!(a.corner_radius, b.corner_radius);
                assert_eq!(a.overall_length, b.overall_length);
                assert_eq!(a.neck_length, b.neck_length);
                assert_eq!(a.stickout, b.stickout);
                assert_eq!(a.holder_diameter, b.holder_diameter);
                assert_eq!(a.presets.len(), b.presets.len());
                for (p, q) in a.presets.iter().zip(&b.presets) {
                    assert_eq!(p.material, q.material);
                    assert!((p.cutting.stepover - q.cutting.stepover).abs() < 1e-9);
                    assert_eq!(p.cutting.feed, q.cutting.feed);
                }
            }
        }
        // 自分の形(PC のライブラリの tools.json)も読める
        let own = serde_json::to_vec(&lib).unwrap();
        assert_eq!(read_library(&own).unwrap().tools, lib.tools);
        // CSV は 1 行が 1 本の 1 つの材料
        let csv = to_csv(&lib);
        let rows: usize = lib.tools.iter().map(|t| t.presets.len()).sum();
        assert_eq!(csv.lines().count(), rows + 1);
        assert!(csv.starts_with("Number (tool_number),Description (tool_description)"));
    }

    #[test]
    fn broken_files_are_refused_with_a_reason() {
        assert!(
            read_library(b"PK\x03\x04garbage")
                .unwrap_err()
                .contains("zip")
        );
        assert!(
            read_library(b"{\"x\": 1}")
                .unwrap_err()
                .contains("工具ライブラリではありません")
        );
        assert!(read_library(b"not json").unwrap_err().contains("JSON"));
        // 中身を書き換えた zip は CRC で分かる
        let z = zip_one("tools.json", b"{\"data\": []}");
        assert_eq!(read_library(&z).unwrap(), Imported::default());
        let mut bad = z.clone();
        // ローカルの見出し(30 バイト)と名前(10 バイト)の後が圧縮した中身
        bad[41] ^= 0xff;
        assert!(read_library(&bad).is_err());
    }
}
