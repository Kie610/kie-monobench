//! 機器から来た報告(HID の入力の報告)を、6 軸の値とボタンに読み解く。OS に依らない純関数。
//!
//! 報告の頭の 1 バイトが報告の番号で、残りが中身。
//! - 報告 1: 移動 X・Y・Z(符号付き 16 ビット、リトルエンディアン)。中身が 12 バイトある機器は、続けて
//!   回転 RX・RY・RZ も同じ報告で送る。
//! - 報告 2: 回転 RX・RY・RZ。
//! - 報告 3: ボタンの押し具合(ビット。長さは機器による)。
//!
//! Windows は報告を機器の一番長い報告の長さに 0 で詰めて渡すので、中身の長さだけでは 6 バイトと
//! 12 バイトを見分けられない。報告 2 を一度でも受けた機器は、報告 1 を移動だけと読む。

use std::time::{Duration, Instant};

/// 手でいっぱいに押した時の値の幅。これで割って ±1 に収め、はみ出しは切る(値の幅の広い機器でも
/// 速くなりすぎない)。
pub const FULL_SCALE: f32 = 350.0;

/// 最後の報告からこれだけ来なければ、手を放したと見て 0 にする(放した時の報告が抜けても回り続けない)。
pub const RELEASE_AFTER: Duration = Duration::from_millis(250);

/// 読み解いた 1 つの報告。
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// 移動 X・Y・Z
    Translate([i16; 3]),
    /// 回転 RX・RY・RZ
    Rotate([i16; 3]),
    /// 移動と回転を 1 つの報告で
    Both([i16; 6]),
    /// 押しているボタン(ビット 0 がボタン 1)
    Buttons(u32),
    /// 知らない番号か長さ
    Unknown { id: u8, len: usize },
}

fn words<const N: usize>(b: &[u8]) -> [i16; N] {
    std::array::from_fn(|i| i16::from_le_bytes([b[2 * i], b[2 * i + 1]]))
}

/// 報告 1 つを読み解く。`rotate_seen` は、この機器から報告 2 を受けたことがあるか。
pub fn parse(report: &[u8], rotate_seen: bool) -> Report {
    let Some((&id, body)) = report.split_first() else {
        return Report::Unknown { id: 0, len: 0 };
    };
    match id {
        1 if body.len() >= 12 && !rotate_seen => Report::Both(words(body)),
        1 if body.len() >= 6 => Report::Translate(words(body)),
        2 if body.len() >= 6 => Report::Rotate(words(body)),
        3 if !body.is_empty() => {
            let mut bits = 0u32;
            for (i, b) in body.iter().take(4).enumerate() {
                bits |= u32::from(*b) << (8 * i);
            }
            Report::Buttons(bits)
        }
        _ => Report::Unknown {
            id,
            len: body.len(),
        },
    }
}

fn scale(v: i16) -> f32 {
    (f32::from(v) / FULL_SCALE).clamp(-1.0, 1.0)
}

/// 1 つの機器の今の状態。報告を受けるたびに [`Axes::apply`] で更新する。
#[derive(Debug, Clone, Default)]
pub struct Axes {
    /// 移動 X・Y・Z と回転 RX・RY・RZ(-1〜1)
    pub values: [f32; 6],
    /// 今押しているボタン(ビット 0 がボタン 1)
    pub held: u32,
    /// 押されてからまだ渡していないボタン(1 から)
    pub pressed: Vec<u8>,
    /// 報告 2 を受けたことがあるか
    pub rotate_seen: bool,
    /// 最後に軸の報告を受けた時刻
    pub last: Option<Instant>,
}

impl Axes {
    /// 報告 1 つを当てる。知らない報告なら何もしない。
    pub fn apply(&mut self, report: &Report, now: Instant) {
        match report {
            Report::Translate(t) => {
                for (v, t) in self.values[..3].iter_mut().zip(t) {
                    *v = scale(*t);
                }
                self.last = Some(now);
            }
            Report::Rotate(r) => {
                self.rotate_seen = true;
                for (v, r) in self.values[3..].iter_mut().zip(r) {
                    *v = scale(*r);
                }
                self.last = Some(now);
            }
            Report::Both(a) => {
                for (v, a) in self.values.iter_mut().zip(a) {
                    *v = scale(*a);
                }
                self.last = Some(now);
            }
            Report::Buttons(bits) => {
                let down = bits & !self.held;
                self.pressed
                    .extend((0..32u8).filter(|i| down & (1 << i) != 0).map(|i| i + 1));
                self.held = *bits;
            }
            Report::Unknown { .. } => {}
        }
    }

    /// `now` の時の 6 軸の値。最後の報告から [`RELEASE_AFTER`] 過ぎていれば 0。
    pub fn at(&self, now: Instant) -> [f32; 6] {
        match self.last {
            Some(t) if now.saturating_duration_since(t) < RELEASE_AFTER => self.values,
            _ => [0.0; 6],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(id: u8, w: &[i16]) -> Vec<u8> {
        let mut v = vec![id];
        for x in w {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v
    }

    /// 確かめること: 6 バイトの移動・回転、12 バイトのまとめた報告、ボタン、はみ出しの切り、
    /// 250 ms 来なければ 0、報告 2 を受けた機器は詰めた報告 1 を移動だけと読む。
    #[test]
    fn decode() {
        let t0 = Instant::now();
        let mut a = Axes::default();
        let r = parse(&bytes(1, &[175, -350, 700]), a.rotate_seen);
        assert_eq!(r, Report::Translate([175, -350, 700]));
        a.apply(&r, t0);
        assert_eq!(a.at(t0)[..3], [0.5, -1.0, 1.0]);

        let both = bytes(1, &[0, 0, 0, 35, 0, -35]);
        assert_eq!(
            parse(&both, false),
            Report::Both([0, 0, 0, 35, 0, -35])
        );
        a.apply(&parse(&both, false), t0);
        assert!((a.at(t0)[3] - 0.1).abs() < 1e-6);
        assert!((a.at(t0)[5] + 0.1).abs() < 1e-6);

        let rot = parse(&bytes(2, &[-700, 0, 1]), a.rotate_seen);
        a.apply(&rot, t0);
        assert!(a.rotate_seen);
        assert_eq!(a.at(t0)[3], -1.0);
        // 報告 2 を受けた後は、0 で詰めた 12 バイトの報告 1 で回転を消さない
        let padded = bytes(1, &[10, 0, 0, 0, 0, 0]);
        assert_eq!(parse(&padded, a.rotate_seen), Report::Translate([10, 0, 0]));
        a.apply(&parse(&padded, a.rotate_seen), t0);
        assert_eq!(a.at(t0)[3], -1.0);

        // ボタン: 押した瞬間だけ 1 回渡す
        a.apply(&parse(&[3, 0b01], false), t0);
        a.apply(&parse(&[3, 0b11], false), t0);
        a.apply(&parse(&[3, 0b10], false), t0);
        assert_eq!(a.pressed, vec![1, 2]);
        assert_eq!(a.held, 0b10);

        // 放した報告が抜けても、250 ms で 0
        assert_ne!(a.at(t0 + Duration::from_millis(200)), [0.0; 6]);
        assert_eq!(a.at(t0 + RELEASE_AFTER), [0.0; 6]);

        assert_eq!(parse(&[9, 1, 2], false), Report::Unknown { id: 9, len: 2 });
        assert_eq!(parse(&[1, 0, 0], false), Report::Unknown { id: 1, len: 2 });
        assert_eq!(parse(&[], false), Report::Unknown { id: 0, len: 0 });
    }
}
