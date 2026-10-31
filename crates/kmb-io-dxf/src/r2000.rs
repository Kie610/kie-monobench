//! 共通の DXF の幾何の書き手に、R2000 のハンドル・所有関係・サブクラスを付ける。

use std::fmt::Write;

struct Writer {
    text: String,
    next: u64,
}

impl Writer {
    fn pair(&mut self, code: i32, value: impl std::fmt::Display) {
        let _ = writeln!(self.text, "{code:>3}\n{value}");
    }
    fn handle(&mut self) -> String {
        let h = format!("{:X}", self.next);
        self.next += 1;
        h
    }
    fn object(&mut self, kind: &str, owner: &str) {
        self.pair(0, kind);
        let h = self.handle();
        self.pair(5, h);
        self.pair(330, owner);
    }
    fn symbol_table(&mut self, name: &str, count: usize) -> String {
        self.pair(0, "TABLE");
        self.pair(2, name);
        let handle = self.handle();
        self.pair(5, &handle);
        self.pair(330, 0);
        self.pair(100, "AcDbSymbolTable");
        self.pair(70, count);
        if name == "DIMSTYLE" {
            self.pair(100, "AcDbDimStyleTable");
            self.pair(71, 0);
        }
        handle
    }

    fn symbol_record(&mut self, kind: &str, owner: &str, name: &str, class: &str) -> String {
        self.pair(0, kind);
        let handle = self.handle();
        self.pair(if kind == "DIMSTYLE" { 105 } else { 5 }, &handle);
        self.pair(330, owner);
        self.pair(100, "AcDbSymbolTableRecord");
        self.pair(100, class);
        self.pair(2, name);
        self.pair(70, 0);
        handle
    }

    fn required_tables(&mut self, lo: [f64; 2], hi: [f64; 2]) {
        let table = self.symbol_table("VPORT", 1);
        self.symbol_record("VPORT", &table, "*ACTIVE", "AcDbViewportTableRecord");
        for (code, value) in [
            (10, 0.0),
            (20, 0.0),
            (11, 1.0),
            (21, 1.0),
            (13, 0.0),
            (23, 0.0),
            (14, 10.0),
            (24, 10.0),
            (15, 10.0),
            (25, 10.0),
            (16, 0.0),
            (26, 0.0),
            (36, 1.0),
            (17, 0.0),
            (27, 0.0),
            (37, 0.0),
            (41, 1.0),
            (42, 50.0),
            (43, 0.0),
            (44, 0.0),
            (50, 0.0),
            (51, 0.0),
        ] {
            self.pair(code, value);
        }
        // 表示の中心と高さも、書き出す図形を包む同じ範囲から決める。
        self.pair(12, lo[0] * 0.5 + hi[0] * 0.5);
        self.pair(22, lo[1] * 0.5 + hi[1] * 0.5);
        let span = (hi[0] - lo[0])
            .max(hi[1] - lo[1])
            .clamp(1.0, f64::MAX / 2.0);
        self.pair(40, span * 1.1);
        for (code, value) in [
            (71, 0),
            (72, 100),
            (73, 1),
            (74, 3),
            (75, 0),
            (76, 0),
            (77, 0),
            (78, 0),
            (281, 0),
        ] {
            self.pair(code, value);
        }
        self.pair(0, "ENDTAB");
        for name in ["VIEW", "UCS"] {
            self.symbol_table(name, 0);
            self.pair(0, "ENDTAB");
        }
        let table = self.symbol_table("APPID", 1);
        self.symbol_record("APPID", &table, "ACAD", "AcDbRegAppTableRecord");
        self.pair(0, "ENDTAB");
        let table = self.symbol_table("STYLE", 1);
        let style = self.symbol_record("STYLE", &table, "STANDARD", "AcDbTextStyleTableRecord");
        for (code, value) in [(40, 0.0), (41, 1.0), (50, 0.0), (42, 2.5)] {
            self.pair(code, value);
        }
        self.pair(71, 0);
        self.pair(3, "txt");
        self.pair(4, "");
        self.pair(0, "ENDTAB");
        let table = self.symbol_table("DIMSTYLE", 1);
        self.symbol_record("DIMSTYLE", &table, "STANDARD", "AcDbDimStyleTableRecord");
        self.pair(3, "");
        self.pair(4, "");
        for (code, value) in [
            (40, 1.0),
            (41, 2.5),
            (42, 0.625),
            (43, 3.75),
            (44, 1.25),
            (45, 0.0),
            (46, 0.0),
            (47, 0.0),
            (48, 0.0),
            (140, 2.5),
            (141, 2.5),
            (142, 0.0),
            (143, 25.4),
            (144, 1.0),
            (145, 0.0),
            (146, 1.0),
            (147, 0.625),
        ] {
            self.pair(code, value);
        }
        for (code, value) in [
            (71, 0),
            (72, 0),
            (73, 0),
            (74, 0),
            (75, 0),
            (76, 0),
            (77, 1),
            (78, 8),
            (170, 0),
            (171, 3),
            (172, 1),
            (173, 0),
            (174, 0),
            (175, 0),
            (176, 0),
            (177, 0),
            (178, 0),
            (179, 0),
            (271, 2),
            (272, 2),
            (273, 2),
            (274, 3),
            (275, 0),
            (276, 0),
            (277, 2),
            (278, 46),
            (279, 0),
            (280, 0),
            (281, 0),
            (282, 0),
            (283, 1),
            (284, 0),
            (285, 0),
            (286, 0),
            (288, 0),
            (289, 3),
        ] {
            self.pair(code, value);
        }
        self.pair(340, style);
        self.pair(371, -2);
        self.pair(372, -2);
        self.pair(0, "ENDTAB");
    }

    fn blocks(&mut self) {
        self.pair(0, "SECTION");
        self.pair(2, "BLOCKS");
        for (name, owner) in [("*Model_Space", "1F"), ("*Paper_Space", "20")] {
            self.object("BLOCK", owner);
            self.pair(100, "AcDbEntity");
            self.pair(8, "0");
            self.pair(100, "AcDbBlockBegin");
            self.pair(2, name);
            self.pair(70, 0);
            for code in [10, 20, 30] {
                self.pair(code, "0.0");
            }
            self.pair(3, name);
            self.pair(1, "");
            self.object("ENDBLK", owner);
            self.pair(100, "AcDbEntity");
            self.pair(8, "0");
            self.pair(100, "AcDbBlockEnd");
        }
        self.pair(0, "ENDSEC");
    }
    fn block_records(&mut self) {
        self.pair(0, "TABLE");
        self.pair(2, "BLOCK_RECORD");
        self.pair(5, "12");
        self.pair(330, 0);
        self.pair(100, "AcDbSymbolTable");
        self.pair(70, 2);
        for (name, handle) in [("*Model_Space", "1F"), ("*Paper_Space", "20")] {
            self.pair(0, "BLOCK_RECORD");
            self.pair(5, handle);
            self.pair(330, "12");
            self.pair(100, "AcDbSymbolTableRecord");
            self.pair(100, "AcDbBlockTableRecord");
            self.pair(2, name);
            self.pair(70, 0);
            self.pair(280, 1);
            self.pair(281, 0);
        }
        self.pair(0, "ENDTAB");
    }
}

pub(crate) fn upgrade(text: &str, lo: [f64; 2], hi: [f64; 2]) -> Result<String, String> {
    let lines: Vec<_> = text.lines().collect();
    let (chunks, remaining) = lines.as_chunks::<2>();
    if !remaining.is_empty() {
        return Err("DXF のグループコードと値の組が途中で終わっています".into());
    }
    let pairs: Vec<(i32, &str)> = chunks
        .iter()
        .enumerate()
        .map(|(i, p)| {
            p[0].trim().parse().map(|code| (code, p[1])).map_err(|_| {
                format!(
                    "DXF の {} 行目のグループコードが数ではありません",
                    2 * i + 1
                )
            })
        })
        .collect::<Result<_, _>>()?;
    if pairs.is_empty() {
        return Err("DXF のグループコードがありません".into());
    }
    let mut records: Vec<&[(i32, &str)]> = Vec::new();
    let mut start = 0;
    for i in 1..pairs.len() {
        if pairs[i].0 == 0 {
            records.push(&pairs[start..i]);
            start = i;
        }
    }
    records.push(&pairs[start..]);
    let mut out = Writer {
        text: String::new(),
        next: 0x30,
    };
    let mut section = "";
    let mut table = String::new();
    for record in records {
        let kind = record[0].1;
        let fields = &record[1..];
        if kind == "SECTION" {
            section = fields.iter().find(|p| p.0 == 2).map_or("", |p| p.1);
            if section == "TABLES" {
                out.pair(0, "SECTION");
                out.pair(2, "CLASSES");
                out.pair(0, "ENDSEC");
            }
            if section == "ENTITIES" {
                out.blocks();
            }
        }
        if kind == "ENDSEC" && section == "TABLES" {
            out.block_records();
            out.required_tables(lo, hi);
        }
        if kind == "EOF" {
            out.pair(0, "SECTION");
            out.pair(2, "OBJECTS");
            out.object("DICTIONARY", "0");
            out.pair(100, "AcDbDictionary");
            out.pair(281, 1);
            out.pair(0, "ENDSEC");
        }
        if kind == "TABLE" {
            out.pair(0, "TABLE");
            out.pair(
                2,
                fields
                    .iter()
                    .find(|p| p.0 == 2)
                    .ok_or("DXF の表の名前がありません")?
                    .1,
            );
            table = out.handle();
            out.pair(5, &table);
            out.pair(330, 0);
            out.pair(100, "AcDbSymbolTable");
            for (code, value) in fields.iter().filter(|p| p.0 != 2) {
                out.pair(*code, *value);
            }
        } else if section == "TABLES" && matches!(kind, "LTYPE" | "LAYER") {
            out.object(kind, &table);
            out.pair(100, "AcDbSymbolTableRecord");
            out.pair(
                100,
                if kind == "LTYPE" {
                    "AcDbLinetypeTableRecord"
                } else {
                    "AcDbLayerTableRecord"
                },
            );
            for (code, value) in fields {
                out.pair(*code, *value);
            }
        } else if section == "ENTITIES" && matches!(kind, "LINE" | "CIRCLE" | "ARC" | "POINT") {
            out.object(kind, "1F");
            out.pair(100, "AcDbEntity");
            out.pair(8, fields.iter().find(|p| p.0 == 8).map_or("0", |p| p.1));
            out.pair(
                100,
                match kind {
                    "LINE" => "AcDbLine",
                    "POINT" => "AcDbPoint",
                    _ => "AcDbCircle",
                },
            );
            let mut arc = false;
            for (code, value) in fields.iter().filter(|p| p.0 != 8) {
                if kind == "ARC" && *code == 50 && !arc {
                    out.pair(100, "AcDbArc");
                    arc = true;
                }
                out.pair(*code, *value);
            }
        } else {
            for (code, value) in record {
                out.pair(
                    *code,
                    if section == "HEADER" && *code == 1 && *value == "AC1009" {
                        "AC1015"
                    } else {
                        value
                    },
                );
            }
        }
    }
    let seed = format!("{:X}", out.next);
    Ok(out.text.replacen(
        "  9\n$ACADVER\n  1\nAC1015\n",
        &format!("  9\n$ACADVER\n  1\nAC1015\n  9\n$HANDSEED\n  5\n{seed}\n"),
        1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_group_codes_are_reported_without_panicking() {
        let result = std::panic::catch_unwind(|| upgrade("bad\nSECTION\n", [0.0; 2], [0.0; 2]));
        let error = result
            .expect("数でないグループコードをpanicにしない")
            .expect_err("不正なコードは誤りにする");
        assert!(
            error.contains("1 行目") && error.contains("グループコード"),
            "{error}"
        );
    }
    #[test]
    fn missing_table_names_are_reported_without_panicking() {
        let result = std::panic::catch_unwind(|| {
            upgrade("  0\nTABLE\n 70\n1\n  0\nEOF\n", [0.0; 2], [0.0; 2])
        });
        let error = result
            .expect("名前のない表をpanicにしない")
            .expect_err("名前のない表は誤りにする");
        assert!(error.contains("表の名前"), "{error}");
    }
}
