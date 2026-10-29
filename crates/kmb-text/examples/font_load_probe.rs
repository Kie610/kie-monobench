//! 日本語の大きなフォントで字の埋め込みと配置操作を測る。普段の検査には入れない。
use kmb_text::{Text, fonts::faces_in, use_face};
use std::path::Path;
use std::time::{Duration, Instant};

fn report(name: &str, mut times: Vec<Duration>) {
    let first = times.remove(0);
    let sum: Duration = times.iter().sum();
    times.sort_unstable();
    println!(
        "{name}: 初回 {:.6} ms / 100回中央値 {:.6} ms / 平均 {:.6} ms",
        first.as_secs_f64() * 1000.0,
        times[times.len() / 2].as_secs_f64() * 1000.0,
        sum.as_secs_f64() * 10.0
    );
}

fn glyph_probe(face: &kmb_text::fonts::Face) {
    let mut times = Vec::new();
    for _ in 0..101 {
        let mut t = Text {
            text: "自作回口の名入れ".into(),
            ..Default::default()
        };
        let start = Instant::now();
        let absent = use_face(&mut t, face).expect("文字の形を入れる");
        let elapsed = start.elapsed();
        assert!(
            absent.is_empty(),
            "日本語の字のあるフォントで測ってください: {absent:?}"
        );
        std::hint::black_box(t);
        times.push(elapsed);
    }
    report("字の埋め込み8字(一覧作成を含まない)", times);
}

/// 指定の書体を候補として1つ足す。OSの候補も含め、実際の配置と同じ順で選ぶ。
struct FontDir(std::path::PathBuf);
impl FontDir {
    fn new(face: &kmb_text::fonts::Face) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("kmb-font-probe-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ext = face.path.extension().unwrap_or(std::ffi::OsStr::new("ttf"));
        std::fs::create_dir_all(dir.join("fonts")).unwrap();
        std::fs::copy(
            &face.path,
            dir.join("fonts")
                .join(Path::new("probe").with_extension(ext)),
        )
        .unwrap();
        Self(dir)
    }
}
impl Drop for FontDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn placement_probe(face: &kmb_text::fonts::Face) {
    use kmb_core::{
        Context, Document, Edit, Extensions, FeatureEntry, Kernel, Op, Outcome, Registry,
        Transaction,
    };
    use kmb_text::{TEXT_AT_POINT, TextCommands};
    use std::sync::Arc;
    let dir = FontDir::new(face);
    let mut index = kmb_text::fonts::FontIndex::default();
    index.add_dir(&dir.0.join("fonts"));
    let started = Instant::now();
    let chosen = index
        .gothic()
        .expect("既定のゴシック体になる書体で測ってください");
    let first_search = started.elapsed();
    assert_eq!(
        chosen.family, face.family,
        "実際に選んだ字体が指定と違います"
    );
    assert_eq!(chosen.style, face.style);
    assert_eq!(chosen.index, face.index);
    println!(
        "配置で選んだ書体: {} / 書体番号 {} / {} bytes",
        chosen.label,
        chosen.index,
        std::fs::metadata(&chosen.path).unwrap().len()
    );
    let started = Instant::now();
    assert!(index.gothic().is_some());
    let memo_search = started.elapsed();
    println!(
        "一覧込みの初回探索 {:.6} ms / 控え後の探索 {:.6} ms",
        first_search.as_secs_f64() * 1000.0,
        memo_search.as_secs_f64() * 1000.0
    );
    let mut reg = Registry::new();
    kmb_sketch::register(&mut reg);
    kmb_text::register(&mut reg);
    let mut sketch = kmb_sketch::Sketch::default();
    sketch.add_point(0.0, 0.0);
    let mut doc = Document::new();
    let sketch_id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "配置の基準",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: sketch_id,
                type_id: kmb_sketch::SKETCH.into(),
                name: "基準".into(),
                data: serde_json::to_value(sketch).unwrap(),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let kernel: Arc<dyn Kernel> = Arc::new(kmb_core::NullKernel);
    let output = doc.recompute(&reg, kernel.as_ref());
    let context = Context {
        selected: Some(sketch_id),
        ..Context::new(doc.data(), &output, &reg, &kernel)
    };
    let text_commands = TextCommands::default();
    kmb_core::Extension::set_data_dir(&text_commands, Some(&dir.0));
    let commands = Extensions(vec![Arc::new(text_commands)]);
    let mut times = Vec::new();
    for _ in 0..101 {
        let started = Instant::now();
        let change = commands.run(TEXT_AT_POINT, &context).expect("文字の配置");
        let elapsed = started.elapsed();
        let Outcome::Change { edits, .. } = &change else {
            panic!("文字を追加する変更")
        };
        let [Edit::Insert { data, .. }] = &edits[..] else {
            panic!("文字1つ")
        };
        let text: Text = serde_json::from_value(data.clone()).unwrap();
        let font = text.outline.as_ref().expect("ゴシック体で置く");
        assert_eq!(font.family, chosen.family);
        assert_eq!(font.style, chosen.style);
        assert!(font.missing(&text.text).is_empty());
        std::hint::black_box(change);
        times.push(elapsed);
    }
    report(
        "配置操作TEXT(既定選択・字形埋め込み・変更の作成、再計算を含まない)",
        times,
    );
}

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .expect("フォントのファイルを指定してください");
    let face = faces_in(Path::new(&path))
        .expect("書体の一覧")
        .into_iter()
        .find(|f| f.embeddable)
        .expect("字の形を入れてよい書体");
    println!(
        "書体: {} / {} / {} / index {} / {} bytes",
        face.family,
        face.style,
        face.label,
        face.index,
        std::fs::metadata(&face.path).unwrap().len()
    );
    if std::env::args_os().skip(2).any(|arg| arg == "--placement") {
        placement_probe(&face);
        return;
    }
    glyph_probe(&face);
    let mut reads = Vec::new();
    let mut opens = Vec::new();
    let mut adds = Vec::new();
    for _ in 0..101 {
        let start = Instant::now();
        let bytes = std::fs::read(&face.path).unwrap();
        reads.push(start.elapsed());
        let start = Instant::now();
        let mut f = kmb_text::OutlineFont::open(&bytes, face.index).unwrap();
        opens.push(start.elapsed());
        let start = Instant::now();
        assert!(
            f.add_glyphs(&bytes, face.index, "自作回口の名入れ")
                .unwrap()
                .is_empty()
        );
        adds.push(start.elapsed());
        std::hint::black_box(f);
    }
    report("内訳: ファイル読取", reads);
    report("内訳: 寸法を読む(open)", opens);
    report("内訳: 字形を入れる(add_glyphs)", adds);
}
