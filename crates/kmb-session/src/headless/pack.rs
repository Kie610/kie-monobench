//! 文書と、役割から呼び出した書き出しを一つのzipにまとめる。
use super::{Headless, Picks, Ran};
use chrono::{DateTime, Utc};
use kmb_core::{FeatureId, FileGate, Problem, ProblemKind};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// まとめて渡す物。保存と同じく文書を変えないセッションの口で扱う。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PackOptions {
    pub gcode: bool,
    pub sheet: bool,
    pub models: bool,
}
impl Default for PackOptions {
    fn default() -> Self {
        Self {
            gcode: true,
            sheet: true,
            models: false,
        }
    }
}

/// zipと、入れたファイル・入れられなかった物の一覧。
#[derive(Debug)]
pub struct Packed {
    pub contents: Vec<u8>,
    pub included: Vec<String>,
    pub skipped: Vec<PackSkipped>,
}
#[derive(Debug, Serialize)]
pub struct PackSkipped {
    pub name: String,
    pub reason: String,
}

/// OSで使えない文字と末尾の点を除き、予約された名前を避ける。
fn file_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    s = s.trim().trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        s = "無題".into();
    }
    let stem = s.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (1..=9).any(|n| stem == format!("COM{n}") || stem == format!("LPT{n}"))
    {
        s.insert(0, '_');
    }
    s
}
fn unique(name: &str, used: &mut BTreeSet<String>) -> String {
    let name = file_name(name);
    let path = Path::new(&name);
    let stem = path.file_stem().and_then(|p| p.to_str()).unwrap_or(&name);
    let ext = path
        .extension()
        .and_then(|p| p.to_str())
        .map_or(String::new(), |s| format!(".{s}"));
    let mut value = name.clone();
    let mut n = 2;
    while !used.insert(value.to_lowercase()) {
        value = format!("{stem}({n}){ext}");
        n += 1;
    }
    value
}

impl Headless {
    /// 文書・G-code・段取り表・任意のSTEPをまとめる。書く場所は呼ぶ側が決める。
    /// `files`は書き出しが読む外部ファイルの許可範囲。zipへリンクの元を同梱するためには読まない。
    pub fn pack(
        &mut self,
        options: PackOptions,
        files: Option<&dyn FileGate>,
    ) -> Result<Packed, Problem> {
        self.pack_at(options, files, Utc::now())
    }

    fn pack_at(
        &mut self,
        options: PackOptions,
        files: Option<&dyn FileGate>,
        now: DateTime<Utc>,
    ) -> Result<Packed, Problem> {
        if self.is_recomputing() || self.needs_recompute() {
            return Err(Problem::new(
                ProblemKind::Unavailable,
                "再計算が終わってからまとめて渡してください",
            ));
        }
        let name = if !self.doc.data().properties.title.is_empty() {
            self.doc.data().properties.title.clone()
        } else {
            self.path
                .as_deref()
                .and_then(Path::file_stem)
                .and_then(|s| s.to_str())
                .unwrap_or("無題")
                .into()
        };
        let name = file_name(&name);
        let mut entries = vec![(
            format!("{name}.{}", kmb_core::DOCUMENT_EXTENSION),
            self.doc.to_json().into_bytes(),
        )];
        let mut skipped = Vec::new();
        // 区分の宣言から探すため、計算できなかったセットアップも理由を残せる。
        let setups: Vec<_> = self
            .doc
            .data()
            .features
            .iter()
            .filter(|f| {
                self.modules
                    .registry
                    .descriptor(&f.type_id)
                    .is_some_and(|d| d.category == kmb_core::category::SETUP)
            })
            .map(|f| (f.id, f.name.clone()))
            .collect();
        let gcode = options.gcode.then(|| {
            self.action_for_role_with(&kmb_interchange::role::action::EXPORT_GCODE, files)
        });
        let sheet = options
            .sheet
            .then(|| self.action_for_role_with(&kmb_interchange::role::action::SETUP_SHEET, files));
        let mut used = BTreeSet::new();
        let mut sheets = Vec::new();
        if setups.is_empty() {
            for (on, name) in [(options.gcode, "G-code"), (options.sheet, "段取り表")] {
                if on {
                    skipped.push(PackSkipped {
                        name: name.into(),
                        reason: "セットアップがありません".into(),
                    });
                }
            }
        }
        for (id, label) in setups {
            for (kind, action, args) in [
                ("G-code", &gcode, json!({})),
                ("段取り表", &sheet, json!({"format":"html"})),
            ] {
                let Some(action) = action else {
                    continue;
                };
                let result = match action {
                    Ok(a) => self.pack_file(a, Some(id), &args, files),
                    Err(e) => Err(e.clone().into()),
                };
                match result {
                    Ok((suffix, data)) if kind == "G-code" => {
                        let ext = Path::new(&suffix)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("nc");
                        let file = unique(&format!("{label}.{ext}"), &mut used);
                        entries.push((format!("gcode/{file}"), data));
                    }
                    Ok((_, data)) => sheets.push((label.clone(), data)),
                    Err(e) => skipped.push(PackSkipped {
                        name: format!("{label}: {kind}"),
                        reason: e.to_string(),
                    }),
                }
            }
        }
        if !sheets.is_empty() {
            let html = if sheets.len() == 1 {
                sheets.remove(0).1
            } else {
                // 文書ごとのスタイルを保ったまま、一つのHTMLで全セットアップを読める。
                let mut html = String::from(
                    "<!doctype html><html lang=\"ja\"><meta charset=\"utf-8\"><title>段取り表</title><body>",
                );
                for (label, data) in sheets {
                    let page =
                        String::from_utf8(data).map_err(|_| "段取り表をUTF-8として読めません")?;
                    html += &format!(
                        "<h1>{}</h1><iframe title=\"{}\" style=\"width:100%;height:90vh;border:0\" srcdoc=\"{}\"></iframe>",
                        escape(&label),
                        escape(&label),
                        escape(&page)
                    );
                }
                html += "</body></html>";
                html.into_bytes()
            };
            entries.push((format!("{name}-段取り表.html"), html));
        }
        if options.models {
            let out = Path::new("models.step");
            let result = self
                .save_action_for_with(out, None, files)
                .map_err(Problem::from)
                .and_then(|action| {
                    let args = json!({"each":true});
                    let picks = Picks {
                        files,
                        ..Picks::default()
                    };
                    self.run_with(&action, &picks, None, &args)
                        .map(|v| v.into_ran())
                });
            match result {
                Ok(Ran::Files { files, .. }) => {
                    let mut used = BTreeSet::new();
                    for (name, data) in files {
                        match step_created_at(data, &now.format("%Y-%m-%dT%H:%M:%S").to_string()) {
                            Ok(data) => {
                                entries.push((format!("models/{}", unique(&name, &mut used)), data))
                            }
                            Err(reason) => skipped.push(PackSkipped { name, reason }),
                        }
                    }
                }
                Ok(_) => skipped.push(PackSkipped {
                    name: "STEP".into(),
                    reason: "ボディごとのファイルが返りませんでした".into(),
                }),
                Err(e) => skipped.push(PackSkipped {
                    name: "STEP".into(),
                    reason: e.to_string(),
                }),
            }
        }
        let mut readme = format!(
            "kieMonoBench {}\n作成日時: {}\n\n入れた物:\n",
            env!("CARGO_PKG_VERSION"),
            now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
        for (name, _) in &entries {
            readme += &format!("- {name}\n");
        }
        readme += "- README.txt\n\n入れなかった物:\n";
        for item in &skipped {
            log::warn!("まとめて渡せない: {}: {}", item.name, item.reason);
            readme += &format!("- {}: {}\n", item.name, item.reason);
        }
        entries.push(("README.txt".into(), readme.into_bytes()));
        if entries.len() >= 65535
            || entries.iter().any(|(n, _)| n.len() > 65535)
            || entries.iter().map(|(_, b)| b.len() as u64).sum::<u64>()
                > kmb_format::zip::UNPACKED_LIMIT as u64
        {
            return Err(Problem::new(
                ProblemKind::Unavailable,
                "まとめるファイルの数・名前か展開後の合計がzipの上限を超えます（合計512 MiBまで）",
            ));
        }
        let included = entries.iter().map(|(n, _)| n.clone()).collect();
        let refs: Vec<_> = entries
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        log::info!(
            "まとめて渡す: ファイル{}個、除外{}件",
            entries.len(),
            skipped.len()
        );
        Ok(Packed {
            contents: kmb_format::zip::write(&refs),
            included,
            skipped,
        })
    }

    fn pack_file(
        &mut self,
        action: &str,
        selected: Option<FeatureId>,
        args: &serde_json::Value,
        files: Option<&dyn FileGate>,
    ) -> Result<(String, Vec<u8>), Problem> {
        let picks = Picks {
            selected,
            files,
            ..Picks::default()
        };
        match self.run_with(action, &picks, None, args)?.into_ran() {
            Ran::File {
                suffix, contents, ..
            } => Ok((suffix, contents)),
            _ => Err(Problem::new(
                ProblemKind::Unavailable,
                "ファイルの書き出しではありません",
            )),
        }
    }
}
/// 書庫の作成日時をSTEPの頭にも使い、個々の書き出しの時計を混ぜない。
fn step_created_at(data: Vec<u8>, stamp: &str) -> Result<Vec<u8>, String> {
    let bad = || "STEPの頭の作成日時を読めません".to_string();
    let mut text = String::from_utf8(data).map_err(|_| bad())?;
    let bytes = text.as_bytes();
    let mut at = text.find("HEADER;").ok_or_else(bad)? + "HEADER;".len();
    let mut start = at;
    let mut quoted = false;
    // 文字列の中のキーワードやセミコロンは、実体の区切りとして読まない。
    let mut at = loop {
        match bytes.get(at) {
            Some(b'\'') if quoted && bytes.get(at + 1) == Some(&b'\'') => {
                at += 2;
                continue;
            }
            Some(b'\'') => quoted = !quoted,
            Some(b';') if !quoted => {
                let record = text[start..at].trim();
                if let Some(args) = record.strip_prefix("FILE_NAME(") {
                    let leading = text[start..at].len() - text[start..at].trim_start().len();
                    break start + leading + record.len() - args.len();
                }
                if record == "ENDSEC" {
                    return Err(bad());
                }
                start = at + 1;
            }
            Some(_) => (),
            None => return Err(bad()),
        }
        at += 1;
    };
    let string = |at: &mut usize| -> Result<std::ops::Range<usize>, String> {
        while bytes.get(*at).is_some_and(u8::is_ascii_whitespace) {
            *at += 1;
        }
        let begin = *at;
        if bytes.get(*at) != Some(&b'\'') {
            return Err(bad());
        }
        *at += 1;
        loop {
            match bytes.get(*at) {
                Some(b'\'') if bytes.get(*at + 1) == Some(&b'\'') => *at += 2,
                Some(b'\'') => {
                    *at += 1;
                    return Ok(begin..*at);
                }
                Some(_) => *at += 1,
                None => return Err(bad()),
            }
        }
    };
    string(&mut at)?;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if bytes.get(at) != Some(&b',') {
        return Err(bad());
    }
    at += 1;
    let time = string(&mut at)?;
    text.replace_range(time, &format!("'{stamp}'"));
    Ok(text.into_bytes())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// ファイルを新しい設計として開く時の結果。書庫からの文書は未保存にする。
pub struct OpenedDesign {
    pub document: kmb_core::Document,
    pub path: Option<PathBuf>,
    pub label: Option<String>,
}

/// 通常の文書か、文書が一つ入ったzipを読む。失敗した時は文書を返さない。
pub fn load_design(path: &Path) -> Result<OpenedDesign, Problem> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        return Ok(OpenedDesign {
            document: crate::load(path)?,
            path: Some(path.to_path_buf()),
            label: None,
        });
    }
    let result = read_zip_design(path);
    if let Err(e) = &result {
        log::error!("書庫を開けない: {}: {e}", path.display());
    }
    result
}

fn read_zip_design(path: &Path) -> Result<OpenedDesign, Problem> {
    let data = std::fs::read(path).map_err(|e| {
        Problem::new(
            ProblemKind::File,
            format!("{} を読めません: {e}", path.display()),
        )
    })?;
    let all = kmb_format::zip::read(&data)?;
    let documents: Vec<_> = all
        .into_iter()
        .filter(|(name, _)| {
            Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| {
                    kmb_core::DOCUMENT_EXTENSIONS
                        .iter()
                        .any(|known| e.eq_ignore_ascii_case(known))
                })
        })
        .collect();
    if documents.len() != 1 {
        return Err(format!(
            "zipの中の文書は一つにしてください（{}個: {}）",
            documents.len(),
            documents
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join("、")
        )
        .into());
    }
    let (name, bytes) = documents.into_iter().next().unwrap();
    let text = std::str::from_utf8(&bytes).map_err(|_| "zipの文書をUTF-8として読めません")?;
    let document = kmb_core::Document::from_json(text).map_err(|e| e.to_string())?;
    let label = Path::new(&name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned());
    log::info!("書庫から開いた: {} / {name}", path.display());
    Ok(OpenedDesign {
        document,
        path: None,
        label,
    })
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
