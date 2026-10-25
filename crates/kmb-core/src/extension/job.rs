//! 時間のかかる仕事の進み具合と結果。

/// 時間のかかる仕事の結果。
#[derive(Debug, Clone, PartialEq)]
pub struct JobReport {
    /// 問題がなかったか(例: シミュレーションで食い込みがない)
    pub ok: bool,
    /// 利用者に見せる文
    pub text: String,
    /// CLI・MCP 向けの詳しい結果(なければ `Null`)
    pub info: serde_json::Value,
}

impl JobReport {
    pub fn failed(text: String) -> Self {
        JobReport {
            ok: false,
            text,
            info: serde_json::Value::Null,
        }
    }
}

/// 時間のかかる仕事。別のスレッドで走らせ、終わると結果を返す。走っている間の進み具合と
/// 中止・仕上げの頼みは [`JobControl`] で受け渡す(見ない仕事は、ただ最後まで走る)。
pub type Job = Box<dyn FnOnce(&JobControl) -> JobReport + Send>;

/// 時間のかかる仕事と画面の間の受け渡し。仕事は [`JobControl::progress`] で進みを知らせ、
/// 区切りごとに [`JobControl::cancelled`]・[`JobControl::finishing`] を見て止まる。
/// 画面は進みを [`JobControl::snapshot`] で読み、中止・仕上げのボタンで頼む。
/// 進みを知らせる仕事は、中止の頼みも見ること(画面は進みを知らせた仕事にだけ中止を出す)。
#[derive(Debug, Default)]
pub struct JobControl {
    cancel: std::sync::atomic::AtomicBool,
    finish: std::sync::atomic::AtomicBool,
    progress: std::sync::Mutex<Option<JobProgress>>,
}

/// 仕事の進み([`JobControl::progress`])。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobProgress {
    /// 0〜1
    pub fraction: f32,
    /// 今の段の短い説明(例: 「光線追跡 24/64 回」)
    pub note: String,
    /// 途中で仕上げられる仕事なら、その操作の名前(例: 「ここで仕上げる」)。
    /// 押すと [`JobControl::finishing`] が真になる
    pub finish: Option<String>,
}

impl JobControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// 進みを知らせる。
    pub fn progress(&self, p: JobProgress) {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    }

    /// 最後に知らせた進み(まだ無ければ `None`)。
    pub fn snapshot(&self) -> Option<JobProgress> {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 中止を頼む(結果を書かずにやめる)。
    pub fn cancel(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// 中止を頼まれたか。
    pub fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 今の所までで仕上げるよう頼む([`JobProgress::finish`] のある仕事)。
    pub fn finish(&self) {
        self.finish
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// 仕上げを頼まれたか。
    pub fn finishing(&self) -> bool {
        self.finish.load(std::sync::atomic::Ordering::Relaxed)
    }
}
