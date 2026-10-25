//! 画面の速さ。フレームごとに、画面全体の処理・3D のビューを描く・面や辺を拾うのにかかった時間を数え、
//! 設定の「画面の速さを出す」が入っていれば 3D のビューの右下に出す。

use std::cell::Cell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// 数える仕事の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// 3D のビューを描く(GPU なら命令を作って送るまで)
    Draw,
    /// 面・辺・点を拾う
    Pick,
}

thread_local! {
    /// 今のフレームで数えた時間(ms)。画面は 1 つのスレッドで描くので、スレッドごとに持つ
    static SPENT: Cell<[f64; 2]> = const { Cell::new([0.0; 2]) };
    /// [`timed`] の入れ子の深さ(内側の時間は外側に入っているので、二重に数えない)
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// 今のフレームの `part` に `ms` を足す。
pub fn add(part: Part, ms: f64) {
    SPENT.with(|s| {
        let mut v = s.get();
        v[part as usize] += ms;
        s.set(v);
    });
}

/// `f` にかかった時間を `part` に足して、`f` の答えを返す。[`timed`] の中の [`timed`] は数えない。
pub fn timed<T>(part: Part, f: impl FnOnce() -> T) -> T {
    let depth = DEPTH.with(|d| d.replace(d.get() + 1));
    let started = Instant::now();
    let out = f();
    DEPTH.with(|d| d.set(depth));
    if depth == 0 {
        add(part, started.elapsed().as_secs_f64() * 1000.0);
    }
    out
}

/// 数えた時間を取り出して 0 に戻す(描く、拾う)。
fn take() -> [f64; 2] {
    SPENT.with(|s| s.replace([0.0; 2]))
}

/// 1 フレームの時間(ms)。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// 画面全体の処理
    pub work: f64,
    pub draw: f64,
    pub pick: f64,
}

/// 直近のフレームを覚えて、fps と時間の平均を出す。
#[derive(Default)]
pub struct Meter {
    /// 直近 1 秒に描き始めたフレームの時刻と時間
    frames: VecDeque<(Instant, Frame)>,
    started: Option<Instant>,
}

/// fps を数える幅。
const WINDOW: Duration = Duration::from_secs(1);

impl Meter {
    /// フレームの始め。前のフレームの残りは捨てる。
    pub fn begin(&mut self, now: Instant) {
        take();
        self.started = Some(now);
    }

    /// フレームの終わり。
    pub fn end(&mut self, now: Instant) {
        let Some(t0) = self.started.take() else {
            return;
        };
        let [draw, pick] = take();
        let work = (now - t0).as_secs_f64() * 1000.0;
        self.frames.push_back((t0, Frame { work, draw, pick }));
        while self
            .frames
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > WINDOW)
        {
            self.frames.pop_front();
        }
    }

    /// 最後に描いたフレームの時間。
    pub fn last(&self) -> Option<Frame> {
        self.frames.back().map(|f| f.1)
    }

    /// 直近 1 秒に描いたフレームの数と、その時間の平均。描いていなければ `None`。
    pub fn summary(&self) -> Option<(usize, Frame)> {
        let n = self.frames.len();
        if n == 0 {
            return None;
        }
        let mut sum = Frame::default();
        for (_, f) in &self.frames {
            sum.work += f.work;
            sum.draw += f.draw;
            sum.pick += f.pick;
        }
        let k = 1.0 / n as f64;
        Some((
            n,
            Frame {
                work: sum.work * k,
                draw: sum.draw * k,
                pick: sum.pick * k,
            },
        ))
    }

    /// 画面に出す文。
    pub fn text(&self) -> Option<String> {
        let (n, f) = self.summary()?;
        Some(format!(
            "{n} fps ・ 1 回の処理 {:.1} ms(描く {:.1}・拾う {:.2})",
            f.work, f.draw, f.pick
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_frames_of_the_last_second_and_averages_their_parts() {
        let mut m = Meter::default();
        let t = Instant::now();
        assert!(m.text().is_none());
        for i in 0..30u64 {
            let at = t + Duration::from_millis(i * 50);
            m.begin(at);
            add(Part::Draw, 2.0);
            add(Part::Pick, 0.5);
            add(Part::Pick, 0.5);
            m.end(at + Duration::from_millis(4));
        }
        // 1.5 秒のうち、終わりから 1 秒に始めたのは 20 か 21 フレーム
        let (n, f) = m.summary().unwrap();
        assert!((20..=21).contains(&n), "{n}");
        assert!((f.work - 4.0).abs() < 1e-6 && f.draw == 2.0 && f.pick == 1.0);
        // フレームの外で数えた時間は、次のフレームに入れない
        add(Part::Draw, 100.0);
        let at = t + Duration::from_millis(1500);
        m.begin(at);
        m.end(at);
        assert!(m.summary().unwrap().1.draw < 2.0);
        assert!(m.text().unwrap().contains("fps"));
    }

    #[test]
    fn nested_timing_is_counted_once() {
        let mut m = Meter::default();
        let t = Instant::now();
        m.begin(t);
        let real = Instant::now();
        timed(Part::Pick, || {
            timed(Part::Pick, || std::thread::sleep(Duration::from_millis(5)))
        });
        // 眠りは混んだ機械では延びるので、決まった上限ではなく実際にかかった時間と比べる
        // (2 回数えると実際の 2 倍になる)
        let real = real.elapsed().as_secs_f64() * 1000.0;
        m.end(t + Duration::from_millis(10));
        let pick = m.summary().unwrap().1.pick;
        assert!(
            pick >= 5.0 && pick < real * 1.5,
            "{pick} ms(実際は {real} ms)"
        );
    }
}
