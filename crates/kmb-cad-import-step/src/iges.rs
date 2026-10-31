//! IGES の中身を文書に持ち、面または立体として再計算する。
use super::{STAND_UP, decode_text};
use kmb_cad::{kernel_err, parse, solid};
use kmb_core::{
    Action, Arg, Context, EvalCtx, EvalError, Extension, Feature, Field, Input, InputRef, Outcome,
    Outputs, Problem, ProblemKind, Registry, Value,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const IMPORT_IGES: &str = "solid.import_iges@1";
pub const IMPORT_IGES_FILE: &str = "cad.import_iges";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportIges {
    #[serde(default)]
    pub file: String,
    pub iges: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stand_up: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub upright: bool,
}
impl ImportIges {
    /// ファイルは操作の許可範囲を確かめてから読む。再計算は保存した中身だけを使う。
    pub fn from_file(path: &Path) -> Result<Self, Problem> {
        let bytes = std::fs::read(path).map_err(|e| {
            Problem::new(
                ProblemKind::File,
                format!("{} を読めません: {e}", path.display()),
            )
        })?;
        let iges = decode_text(&bytes);
        let metadata = kmb_format::iges::read(&iges).map_err(|why| {
            Problem::new(
                ProblemKind::Input,
                format!("{} の IGES を読めません: {why}", path.display()),
            )
        })?;
        Ok(Self {
            file: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            upright: super::names::our_producer(&metadata.global.producer),
            iges,
            stand_up: false,
        })
    }
}
impl Feature for ImportIges {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(Vec::new())
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let (mut body, warnings) = ctx.kernel().import_iges(&self.iges).map_err(kernel_err)?;
        if self.stand_up {
            body = ctx
                .kernel()
                .transform(body, &STAND_UP, "")
                .map_err(kernel_err)?;
        }
        let mut out: Outputs = [("body".to_string(), Value::Shape(body))].into();
        for warning in &warnings {
            log::warn!("IGES: {warning}");
        }
        if !warnings.is_empty() {
            kmb_core::add_warnings(&mut out, warnings);
        }
        Ok(out)
    }
}
pub(super) fn register(reg: &mut Registry) {
    reg.register_described(
        solid(IMPORT_IGES, "IGES の読み込み").icon("iges")
            .hint("IGES の面と立体を読み込む。閉じない面は面のボディとして残し、読めない面の理由を知らせる")
            .field(Field::info("file", "ファイル"))
            .field(Field::bool("stand_up", "Y が上の形を立てる").help("Y が上で作られた形を、X 軸のまわりに 90 度回して Z が上に立てる"))
            .no_raw_editor(),
        |d| Ok(Box::new(parse::<ImportIges>(d)?)),
    );
}
pub struct ImportIgesCommands;
impl Extension for ImportIgesCommands {
    fn actions(&self, _: &Context<'_>) -> Vec<Action> {
        vec![Action::new(IMPORT_IGES_FILE, "IGES を読み込む…", "cad")
            .role(kmb_interchange::role::action::IMPORT_IGES)
            .arg(Arg::file("path", "IGES のファイル(省くとファイルを選ぶ窓を出す)"))
            .menu("挿入").icon("iges").aliases("よみこみ import iges igs")
            .hint("IGES の面と立体を設計へ挿入する。元のファイルが無くても開けるよう中身を文書に残す")
            .input(Input::OpenFile { title:"IGES を読み込む".into(), filter:"IGES".into(), extensions:["igs","iges","IGS","IGES"].map(String::from).to_vec() })]
    }
    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        if id != IMPORT_IGES_FILE {
            return None;
        }
        Some((|| {
            let path = cx
                .arg("path")
                .and_then(|v| v.as_str())
                .map(Path::new)
                .or(cx.file)
                .ok_or("ファイルを選んでいません")?;
            let path = cx
                .open_path(path)
                .map_err(|why| Problem::new(ProblemKind::File, why))?;
            let data = ImportIges::from_file(&path)?;
            let name = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "読み込んだ部品".into());
            Ok(Outcome::insert(
                "IGES を読み込む",
                IMPORT_IGES,
                name,
                serde_json::to_value(data).expect("文字と印はJSONにできる"),
            ))
        })())
    }
}
