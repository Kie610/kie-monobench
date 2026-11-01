//! SOFA の書き出しに要るだけの HDF5 の書き込み。
//!
//! SOFA の読み手は、次元(M・R・N など)を「次元の目盛りのデータ」と、各データの `DIMENSION_LIST`
//! (可変長の参照の並び)の属性で見つける。純 Rust の HDF5 の crate は可変長の参照の属性を書けない
//! ので、SOFA に要る形だけを自前で書く。書くのは次の物に限る。
//!
//! - 超ブロック 2 版、オブジェクトの頭は 2 版(検査の和つき)。
//! - 根のグループ 1 つ。リンクは密な置き方(断片のヒープ + 名前の索引の B 木 2 版)。軽い読み手は
//!   リンクを断片のヒープの直のブロックからしか読まないので、短い置き方(頭の中のリンク)は使わない。
//! - データは連続の置き方の f64・f32 だけ。属性は文字・次元の参照(`DIMENSION_LIST`)・参照の一覧
//!   (`REFERENCE_LIST`)。
//!
//! 参照は相手の頭の番地を書くので、先に全部の大きさを測って番地を決め、そのあと中身を書く。

/// データの値。
pub enum Values {
    F64(Vec<f64>),
    F32(Vec<f32>),
}

impl Values {
    fn len(&self) -> usize {
        match self {
            Values::F64(v) => v.len(),
            Values::F32(v) => v.len(),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Values::F64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Values::F32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        }
    }

    fn datatype(&self) -> Vec<u8> {
        match self {
            Values::F64(_) => float_type(8, 63, 52, 11, 52, 1023),
            Values::F32(_) => float_type(4, 31, 23, 8, 23, 127),
        }
    }
}

/// 属性の値。
pub enum Attr {
    Text(String),
    /// 次の並びの番号のデータ(次元の目盛り)を、次元の順に指す。
    DimensionList(Vec<usize>),
    /// (指すデータの番号, そのデータの何番目の次元か)の並び。
    ReferenceList(Vec<(usize, i32)>),
}

/// 根のグループに置くデータ。
pub struct Dataset {
    pub name: String,
    pub shape: Vec<u64>,
    pub values: Values,
    pub attrs: Vec<(String, Attr)>,
}

/// 書けない時の理由。
#[derive(Debug, PartialEq, Eq)]
pub struct WriteError(pub String);

const UNDEF: u64 = u64::MAX;
const SUPERBLOCK: usize = 48;
const HEAP_HEADER: usize = 4 + 1 + 2 + 2 + 1 + 4 + 8 * 12 + 2 + 8 + 8 + 2 + 2 + 8 + 2 + 4;
const DIRECT_HEADER: usize = 4 + 1 + 8 + 4 + 4;
const BTREE_HEADER: usize = 4 + 1 + 1 + 4 + 2 + 2 + 1 + 1 + 8 + 2 + 8 + 4;
const BTREE_RECORD: usize = 4 + 7;
const GCOL_HEADER: usize = 16;
const GCOL_OBJECT: usize = 16 + 8;

/// 根の属性 `root_attrs` とデータの並び `sets` を、HDF5 のファイルのバイト列にする。
pub fn write(root_attrs: &[(String, String)], sets: &[Dataset]) -> Result<Vec<u8>, WriteError> {
    for s in sets {
        let n: u64 = s.shape.iter().product();
        if n as usize != s.values.len() {
            return Err(WriteError(format!(
                "{}: 形 {:?} と値の数 {} が合わない",
                s.name,
                s.shape,
                s.values.len()
            )));
        }
        if s.name.is_empty() || s.name.len() > 255 {
            return Err(WriteError(format!(
                "名前「{}」の長さが 1〜255 でない",
                s.name
            )));
        }
    }
    // 次元の参照ごとに大域のヒープの物を 1 つ置く(番号は 1 から)。
    let mut gcol_refs: Vec<usize> = Vec::new();
    for s in sets {
        for (_, a) in &s.attrs {
            if let Attr::DimensionList(list) = a {
                for &i in list {
                    if i >= sets.len() {
                        return Err(WriteError(format!("{}: 次元の番号 {i} が無い", s.name)));
                    }
                    gcol_refs.push(i);
                }
            }
        }
    }
    if gcol_refs.len() > u16::MAX as usize {
        return Err(WriteError("次元の参照が多すぎる".into()));
    }
    let gcol_size = (GCOL_HEADER + gcol_refs.len() * GCOL_OBJECT + 16).max(4096);

    let links: Vec<usize> = sets
        .iter()
        .map(|s| 1 + 1 + 8 + 1 + s.name.len() + 8)
        .collect();
    let used: usize = links.iter().sum();
    // 後ろに 0 の 1 バイトを残す(軽い読み手は 0 で物の終わりを知る)。
    let block = (DIRECT_HEADER + used + 1).next_power_of_two().max(512);
    if block > 65536 {
        return Err(WriteError(
            "データが多すぎてリンクの置き場に収まらない".into(),
        ));
    }
    let node = (10 + sets.len() * BTREE_RECORD)
        .next_power_of_two()
        .max(512);

    // 番地を決める。頭の大きさは番地の値によらないので、仮の番地で 1 回組んで測る。
    let zero = vec![0u64; sets.len()];
    let mut ctx = Ctx {
        heap: 0,
        btree: 0,
        gcol: 0,
        heads: zero.clone(),
        data: zero,
    };
    let root_len = root_header(root_attrs, &ctx)?.len();
    let mut at = SUPERBLOCK + root_len;
    ctx.heap = at as u64;
    at += HEAP_HEADER;
    let direct = at;
    at += block;
    ctx.btree = at as u64;
    at += BTREE_HEADER;
    let leaf = at;
    at += node;
    ctx.gcol = at as u64;
    at += gcol_size;
    let mut head_len = Vec::with_capacity(sets.len());
    for s in sets.iter() {
        head_len.push(dataset_header(s, &ctx, 0, &mut 1)?.len());
    }
    for (i, s) in sets.iter().enumerate() {
        at = align8(at);
        ctx.heads[i] = at as u64;
        at += head_len[i];
        at = align8(at);
        ctx.data[i] = at as u64;
        at += s.values.len()
            * if matches!(s.values, Values::F64(_)) {
                8
            } else {
                4
            };
    }
    let eof = at;

    let mut out = vec![0u8; eof];
    // 超ブロック。
    let mut sb = Vec::with_capacity(SUPERBLOCK);
    sb.extend_from_slice(b"\x89HDF\r\n\x1a\n");
    sb.extend_from_slice(&[2, 8, 8, 0]);
    sb.extend_from_slice(&0u64.to_le_bytes());
    sb.extend_from_slice(&UNDEF.to_le_bytes());
    sb.extend_from_slice(&(eof as u64).to_le_bytes());
    sb.extend_from_slice(&(SUPERBLOCK as u64).to_le_bytes());
    let c = lookup3(&sb);
    sb.extend_from_slice(&c.to_le_bytes());
    put(&mut out, 0, &sb);
    put(&mut out, SUPERBLOCK, &root_header(root_attrs, &ctx)?);

    // 断片のヒープ: 頭と直のブロック 1 つ。リンクは作った順に詰める。
    let mut ids = Vec::with_capacity(sets.len());
    let mut blk = vec![0u8; block];
    blk[..4].copy_from_slice(b"FHDB");
    blk[4] = 0;
    blk[5..13].copy_from_slice(&ctx.heap.to_le_bytes());
    // 13..17 はブロックの位置(0)、17..21 は検査の和。
    let mut p = DIRECT_HEADER;
    for (i, s) in sets.iter().enumerate() {
        let mut l = vec![1u8, 0x04];
        l.extend_from_slice(&(i as u64).to_le_bytes());
        l.push(s.name.len() as u8);
        l.extend_from_slice(s.name.as_bytes());
        l.extend_from_slice(&ctx.heads[i].to_le_bytes());
        debug_assert_eq!(l.len(), links[i]);
        blk[p..p + l.len()].copy_from_slice(&l);
        let mut id = vec![0u8];
        id.extend_from_slice(&(p as u32).to_le_bytes());
        id.extend_from_slice(&(l.len() as u16).to_le_bytes());
        ids.push(id);
        p += l.len();
    }
    let c = lookup3(&blk);
    blk[17..21].copy_from_slice(&c.to_le_bytes());
    put(&mut out, direct, &blk);

    let mut h = Vec::with_capacity(HEAP_HEADER);
    h.extend_from_slice(b"FRHP");
    h.push(0);
    h.extend_from_slice(&7u16.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h.push(0x02);
    h.extend_from_slice(&4096u32.to_le_bytes());
    for v in [
        0,
        UNDEF,
        (block - DIRECT_HEADER - used) as u64,
        UNDEF,
        block as u64,
        block as u64,
        block as u64,
        sets.len() as u64,
        0,
        0,
        0,
        0,
    ] {
        h.extend_from_slice(&v.to_le_bytes());
    }
    h.extend_from_slice(&4u16.to_le_bytes());
    h.extend_from_slice(&(block as u64).to_le_bytes());
    h.extend_from_slice(&65536u64.to_le_bytes());
    h.extend_from_slice(&32u16.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&(direct as u64).to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    let c = lookup3(&h);
    h.extend_from_slice(&c.to_le_bytes());
    debug_assert_eq!(h.len(), HEAP_HEADER);
    put(&mut out, ctx.heap as usize, &h);

    // 名前の索引の B 木(深さ 0、葉 1 枚)。名前の検査の和の順に並べる。
    let mut recs: Vec<(u32, &[u8])> = sets
        .iter()
        .zip(&ids)
        .map(|(s, id)| (lookup3(s.name.as_bytes()), id.as_slice()))
        .collect();
    recs.sort_by_key(|r| r.0);
    let mut lf = Vec::with_capacity(node);
    lf.extend_from_slice(b"BTLF");
    lf.extend_from_slice(&[0, 5]);
    for (hash, id) in &recs {
        lf.extend_from_slice(&hash.to_le_bytes());
        lf.extend_from_slice(id);
    }
    let c = lookup3(&lf);
    lf.extend_from_slice(&c.to_le_bytes());
    put(&mut out, leaf, &lf);
    let mut bt = Vec::with_capacity(BTREE_HEADER);
    bt.extend_from_slice(b"BTHD");
    bt.extend_from_slice(&[0, 5]);
    bt.extend_from_slice(&(node as u32).to_le_bytes());
    bt.extend_from_slice(&(BTREE_RECORD as u16).to_le_bytes());
    bt.extend_from_slice(&0u16.to_le_bytes());
    bt.extend_from_slice(&[100, 40]);
    bt.extend_from_slice(&(leaf as u64).to_le_bytes());
    bt.extend_from_slice(&(sets.len() as u16).to_le_bytes());
    bt.extend_from_slice(&(sets.len() as u64).to_le_bytes());
    let c = lookup3(&bt);
    bt.extend_from_slice(&c.to_le_bytes());
    debug_assert_eq!(bt.len(), BTREE_HEADER);
    put(&mut out, ctx.btree as usize, &bt);

    // 大域のヒープ: 次元の参照を 1 つずつ。残りは空きの物(番号 0)。
    let mut g = Vec::with_capacity(gcol_size);
    g.extend_from_slice(b"GCOL");
    g.extend_from_slice(&[1, 0, 0, 0]);
    g.extend_from_slice(&(gcol_size as u64).to_le_bytes());
    for (k, &i) in gcol_refs.iter().enumerate() {
        g.extend_from_slice(&((k + 1) as u16).to_le_bytes());
        g.extend_from_slice(&0u16.to_le_bytes());
        g.extend_from_slice(&0u32.to_le_bytes());
        g.extend_from_slice(&8u64.to_le_bytes());
        g.extend_from_slice(&ctx.heads[i].to_le_bytes());
    }
    let free = gcol_size - g.len();
    g.extend_from_slice(&[0; 8]);
    g.extend_from_slice(&(free as u64).to_le_bytes());
    put(&mut out, ctx.gcol as usize, &g);

    let mut next = 1u32;
    for (i, s) in sets.iter().enumerate() {
        put(
            &mut out,
            ctx.heads[i] as usize,
            &dataset_header(s, &ctx, i, &mut next)?,
        );
        put(&mut out, ctx.data[i] as usize, &s.values.bytes());
    }
    Ok(out)
}

struct Ctx {
    heap: u64,
    btree: u64,
    gcol: u64,
    heads: Vec<u64>,
    data: Vec<u64>,
}

fn align8(x: usize) -> usize {
    x.div_ceil(8) * 8
}

fn put(out: &mut [u8], at: usize, bytes: &[u8]) {
    out[at..at + bytes.len()].copy_from_slice(bytes);
}

fn root_header(attrs: &[(String, String)], ctx: &Ctx) -> Result<Vec<u8>, WriteError> {
    let mut li = vec![0u8, 0x01];
    li.extend_from_slice(&(ctx.heads.len() as u64).to_le_bytes());
    li.extend_from_slice(&ctx.heap.to_le_bytes());
    li.extend_from_slice(&ctx.btree.to_le_bytes());
    let mut msgs = vec![(2u8, 0u8, li), (10, 0, vec![0, 0])];
    for (k, v) in attrs {
        msgs.push((
            12,
            0,
            attribute(k, &text_type(v), &scalar(), &text_bytes(v))?,
        ));
    }
    header(&msgs)
}

fn dataset_header(
    s: &Dataset,
    ctx: &Ctx,
    me: usize,
    next: &mut u32,
) -> Result<Vec<u8>, WriteError> {
    let mut layout = vec![3u8, 1];
    layout.extend_from_slice(&ctx.data[me].to_le_bytes());
    let bytes = s.values.len()
        * if matches!(s.values, Values::F64(_)) {
            8
        } else {
            4
        };
    layout.extend_from_slice(&(bytes as u64).to_le_bytes());
    let mut msgs = vec![
        (1u8, 0u8, simple(&s.shape)),
        (3, 1, s.values.datatype()),
        (5, 1, vec![3, 0x09]),
        (8, 0, layout),
    ];
    for (k, a) in &s.attrs {
        let body = match a {
            Attr::Text(v) => attribute(k, &text_type(v), &scalar(), &text_bytes(v))?,
            Attr::DimensionList(list) => {
                let mut data = Vec::with_capacity(list.len() * 16);
                for _ in list {
                    data.extend_from_slice(&1u32.to_le_bytes());
                    data.extend_from_slice(&ctx.gcol.to_le_bytes());
                    data.extend_from_slice(&next.to_le_bytes());
                    *next += 1;
                }
                let mut vlen = vec![0x19, 0, 0, 0];
                vlen.extend_from_slice(&16u32.to_le_bytes());
                vlen.extend_from_slice(&ref_type());
                attribute(k, &vlen, &simple(&[list.len() as u64]), &data)?
            }
            Attr::ReferenceList(list) => {
                let mut data = Vec::with_capacity(list.len() * 12);
                for &(i, dim) in list {
                    let addr = *ctx
                        .heads
                        .get(i)
                        .ok_or_else(|| WriteError(format!("{}: 参照の番号 {i} が無い", s.name)))?;
                    data.extend_from_slice(&addr.to_le_bytes());
                    data.extend_from_slice(&dim.to_le_bytes());
                }
                let mut comp = vec![0x36, 2, 0, 0];
                comp.extend_from_slice(&12u32.to_le_bytes());
                comp.extend_from_slice(b"dataset\0");
                comp.push(0);
                comp.extend_from_slice(&ref_type());
                comp.extend_from_slice(b"dimension\0");
                comp.push(8);
                comp.extend_from_slice(&[0x10, 0x08, 0, 0]);
                comp.extend_from_slice(&4u32.to_le_bytes());
                comp.extend_from_slice(&0u16.to_le_bytes());
                comp.extend_from_slice(&32u16.to_le_bytes());
                attribute(k, &comp, &simple(&[list.len() as u64]), &data)?
            }
        };
        msgs.push((12, 0, body));
    }
    header(&msgs)
}

/// オブジェクトの頭(2 版、塊 0 の大きさは 4 バイト)。
fn header(msgs: &[(u8, u8, Vec<u8>)]) -> Result<Vec<u8>, WriteError> {
    let mut body = Vec::new();
    for (kind, flags, data) in msgs {
        let n = u16::try_from(data.len())
            .map_err(|_| WriteError("頭の中の 1 件が 64 KiB を超える(属性が長すぎる)".into()))?;
        body.push(*kind);
        body.extend_from_slice(&n.to_le_bytes());
        body.push(*flags);
        body.extend_from_slice(data);
    }
    let mut h = b"OHDR".to_vec();
    h.extend_from_slice(&[2, 0x02]);
    h.extend_from_slice(&(body.len() as u32).to_le_bytes());
    h.extend_from_slice(&body);
    let c = lookup3(&h);
    h.extend_from_slice(&c.to_le_bytes());
    Ok(h)
}

fn attribute(name: &str, dt: &[u8], ds: &[u8], data: &[u8]) -> Result<Vec<u8>, WriteError> {
    let mut a = vec![3u8, 0];
    let len = u16::try_from(name.len() + 1)
        .map_err(|_| WriteError(format!("属性の名前「{name}」が長すぎる")))?;
    a.extend_from_slice(&len.to_le_bytes());
    a.extend_from_slice(&(dt.len() as u16).to_le_bytes());
    a.extend_from_slice(&(ds.len() as u16).to_le_bytes());
    a.push(u8::from(!name.is_ascii()));
    a.extend_from_slice(name.as_bytes());
    a.push(0);
    a.extend_from_slice(dt);
    a.extend_from_slice(ds);
    a.extend_from_slice(data);
    Ok(a)
}

fn scalar() -> Vec<u8> {
    vec![2, 0, 0, 0]
}

fn simple(shape: &[u64]) -> Vec<u8> {
    let mut d = vec![2, shape.len() as u8, 0, 1];
    for n in shape {
        d.extend_from_slice(&n.to_le_bytes());
    }
    d
}

/// 0 で終わる決まった長さの文字(ASCII でなければ UTF-8 の印を付ける)。
fn text_type(v: &str) -> Vec<u8> {
    let charset = if v.is_ascii() { 0x00 } else { 0x10 };
    let mut t = vec![0x13, charset, 0, 0];
    t.extend_from_slice(&((v.len() + 1) as u32).to_le_bytes());
    t
}

fn text_bytes(v: &str) -> Vec<u8> {
    let mut b = v.as_bytes().to_vec();
    b.push(0);
    b
}

fn ref_type() -> Vec<u8> {
    let mut t = vec![0x17, 0, 0, 0];
    t.extend_from_slice(&8u32.to_le_bytes());
    t
}

fn float_type(size: u32, sign: u8, exp_at: u8, exp_bits: u8, mant_bits: u8, bias: u32) -> Vec<u8> {
    let mut t = vec![0x11, 0x20, sign, 0];
    t.extend_from_slice(&size.to_le_bytes());
    t.extend_from_slice(&0u16.to_le_bytes());
    t.extend_from_slice(&((size * 8) as u16).to_le_bytes());
    t.extend_from_slice(&[exp_at, exp_bits, 0, mant_bits]);
    t.extend_from_slice(&bias.to_le_bytes());
    t
}

/// HDF5 の検査の和(Jenkins の lookup3、初めの値 0)。
pub fn lookup3(key: &[u8]) -> u32 {
    let mut a = 0xdead_beefu32.wrapping_add(key.len() as u32);
    let mut b = a;
    let mut c = a;
    let word = |k: &[u8]| -> u32 {
        k.iter()
            .enumerate()
            .fold(0u32, |s, (i, &x)| s.wrapping_add((x as u32) << (8 * i)))
    };
    let mut k = key;
    while k.len() > 12 {
        a = a.wrapping_add(word(&k[0..4]));
        b = b.wrapping_add(word(&k[4..8]));
        c = c.wrapping_add(word(&k[8..12]));
        // mix
        a = a.wrapping_sub(c);
        a ^= c.rotate_left(4);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= a.rotate_left(6);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= b.rotate_left(8);
        b = b.wrapping_add(a);
        a = a.wrapping_sub(c);
        a ^= c.rotate_left(16);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= a.rotate_left(19);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= b.rotate_left(4);
        b = b.wrapping_add(a);
        k = &k[12..];
    }
    if k.is_empty() {
        return c;
    }
    a = a.wrapping_add(word(&k[..k.len().min(4)]));
    if k.len() > 4 {
        b = b.wrapping_add(word(&k[4..k.len().min(8)]));
    }
    if k.len() > 8 {
        c = c.wrapping_add(word(&k[8..]));
    }
    // final
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(14));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(11));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(25));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(16));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(4));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(14));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(24));
    c
}
