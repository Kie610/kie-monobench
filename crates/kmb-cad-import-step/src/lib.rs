//! STEP の読み込み(`solid.import_step@1`)。STEP の中身を文書に入れて持つので、元の
//! ファイルを動かしたり消したりしても文書は開ける。読み込んだ形は加工と計測に使え、カーネルが立体に
//! 組めれば足し引きやフィレットにも使える(truck のカーネルと、組めない形では使えない)。

use kmb_cad::kernel_err;
use kmb_cad::{parse, solid};
use kmb_core::{EvalCtx, EvalError, Feature, InputRef, Outputs, Value};
use kmb_core::{Field, Registry};
use serde::{Deserialize, Serialize};

mod names;
pub use names::{import_name, product_names, written_by_jc};

pub const IMPORT_STEP: &str = "solid.import_step@1";
/// STEP のファイルを選んで読み込む操作。
pub const IMPORT_STEP_FILE: &str = "cad.import_step";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportStep {
    /// 元のファイルの名前(表示用。読み直しには使わない)
    #[serde(default)]
    pub file: String,
    /// STEP の中身
    pub step: String,
    /// Y が上で作られた形を、X 軸のまわりに 90 度回して Z が上に立てる(U12)。
    /// 他の CAD の多くは Y が上で、そのまま読むと寝た向きで入る
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stand_up: bool,
    /// 書いた CAD が Z を上にして書いた(JC の STEP)。読み込んだ後に立てるかを聞かない(U12)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub upright: bool,
}

/// STEP ファイルの字を読む。UTF-8(頭の BOM は外す)でなければ Shift_JIS(日本の CAD は名前を
/// そのままの字で書くことがある)、それでも読めなければ ISO 8859-1 とみなす(どのバイトも 1 字に
/// なる。形を決める所は ASCII だけなので、名前の字が化けても形は読める)。
pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (s, _, bad) = encoding_rs::SHIFT_JIS.decode(bytes);
    if !bad {
        log::info!("STEP を Shift_JIS として読む");
        return s.into_owned();
    }
    log::info!("STEP の字が UTF-8 でも Shift_JIS でもないので、ISO 8859-1 として読む");
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// Y が上の形を Z が上に立てる回し方(X 軸のまわりに +90 度。Y の向きが Z の向きになる)。
pub const STAND_UP: kmb_core::kernel::Matrix4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, -1.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

impl ImportStep {
    /// ファイルを読んで機能のデータを作る。
    pub fn from_file(path: &std::path::Path) -> Result<ImportStep, kmb_core::Problem> {
        let bytes = std::fs::read(path).map_err(|e| {
            kmb_core::Problem::new(
                kmb_core::ProblemKind::File,
                format!("{} を読めません: {e}", path.display()),
            )
        })?;
        let step = decode_text(&bytes);
        if !step.trim_start().starts_with("ISO-10303-21") {
            return Err(format!("{} は STEP(ISO-10303-21)ではありません", path.display()).into());
        }
        Ok(ImportStep {
            file: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            upright: written_by_jc(&step),
            step,
            stand_up: false,
        })
    }
}

impl Feature for ImportStep {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut body = ctx.kernel().import_step(&self.step).map_err(kernel_err)?;
        if self.stand_up {
            body = ctx
                .kernel()
                // 面の名前は変えない(加工や測る道具が読み込んだ面を名前で指しているため)
                .transform(body, &STAND_UP, "")
                .map_err(kernel_err)?;
        }
        Ok([("body".to_string(), Value::Shape(body))].into())
    }
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(IMPORT_STEP, "STEP の読み込み")
            .icon("step")
            .hint("読み込んだ形に穴・ポケット・フィレットを足せる。立体に組めない形は網で読み、加工と計測だけに使える")
            .field(Field::info("file", "ファイル"))
            .field(
                Field::bool("stand_up", "Y が上の形を立てる").help(
                    "他の CAD(Y が上)で作った形を、X 軸のまわりに 90 度回して Z が上に立てる",
                ),
            )
            .no_raw_editor(),
        |d| Ok(Box::new(parse::<ImportStep>(d)?)),
    );
}

use kmb_core::{Action, Context, Extension, Input, Outcome, Place};

/// STEP のファイルを読み込む機能を足す(中身は文書に入れる)。
fn import_step(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let path = cx.file.ok_or("ファイルを選んでいません")?;
    let data = ImportStep::from_file(path)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "読み込んだ部品".into());
    // 部品が 1 つなら、書いた CAD の部品の名前を使う
    let name = import_name(&data.step, &stem);
    Ok(Outcome::insert(
        "STEP を読み込む",
        IMPORT_STEP,
        name,
        serde_json::to_value(data).expect("JSON にできる"),
    ))
}

/// STEP を読み込む操作。
pub struct ImportStepCommands;

impl Extension for ImportStepCommands {
    fn actions(&self, _cx: &Context<'_>) -> Vec<Action> {
        vec![
            Action::new(IMPORT_STEP_FILE, "STEP を読み込む…", "cad")
                .role(kmb_interchange::role::action::IMPORT_STEP)
                .menu("挿入")
                .icon("step")
                .aliases("よみこみ import step stp")
                .hint("他の CAD で作った部品を読み込む。穴やポケットを足し、加工と計測に使える")
                .at(&[Place::Toolbar])
                .input(Input::OpenFile {
                    title: "STEP を読み込む".into(),
                    filter: "STEP".into(),
                    extensions: ["step", "stp", "STEP", "STP"].map(String::from).to_vec(),
                }),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        Some(match id {
            IMPORT_STEP_FILE => import_step(cx),
            _ => return None,
        })
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-import-step", register).with_extension(ImportStepCommands)
}

#[cfg(test)]
mod text_tests {
    use super::*;

    #[test]
    fn step_text_in_shift_jis_or_with_a_bom_keeps_its_japanese_names() {
        let head = "ISO-10303-21;\nDATA;\n#1=PRODUCT('";
        let tail = "','','',(#2));\nENDSEC;\n";
        // Shift_JIS の「ブラケット」をそのまま書いた STEP(前は UTF-8 でないので読めなかった)
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("ブラケット");
        let bytes = [head.as_bytes(), &sjis, tail.as_bytes()].concat();
        assert!(std::str::from_utf8(&bytes).is_err());
        let step = decode_text(&bytes);
        assert_eq!(names::product_names(&step), vec!["ブラケット"]);
        // UTF-8 の頭の BOM は外す(外さないと「ISO-10303-21」で始まらない)
        let bom = [
            b"\xEF\xBB\xBF".as_slice(),
            head.as_bytes(),
            "台".as_bytes(),
            tail.as_bytes(),
        ]
        .concat();
        let step = decode_text(&bom);
        assert!(step.starts_with("ISO-10303-21"));
        assert_eq!(names::product_names(&step), vec!["台"]);
        // どちらでもない字は 1 バイト 1 字で読み、形の所(ASCII)は変えない
        let odd = [head.as_bytes(), &[0x81, 0x7f], tail.as_bytes()].concat();
        assert!(decode_text(&odd).ends_with(tail));
    }
}
