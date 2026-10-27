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

pub(crate) fn upgrade(text: &str) -> String {
    let lines: Vec<_> = text.lines().collect();
    let pairs: Vec<(i32, &str)> = lines
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| (p[0].trim().parse().expect("書き手のグループコード"), p[1]))
        .collect();
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
            if section == "ENTITIES" {
                out.blocks();
            }
        }
        if kind == "ENDSEC" && section == "TABLES" {
            out.block_records();
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
            out.pair(2, fields.iter().find(|p| p.0 == 2).expect("表の名前").1);
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
    out.text.replacen(
        "  9\n$ACADVER\n  1\nAC1015\n",
        &format!("  9\n$ACADVER\n  1\nAC1015\n  9\n$HANDSEED\n  5\n{seed}\n"),
        1,
    )
}
