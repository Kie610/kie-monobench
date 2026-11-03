//! 文書を変えず、Headless の派生データの仕事でボディを測る。

use super::*;
use kmb_core::{
    DocumentData, JobControl, JobProgress, KernelError, Problem, ProblemKind, ShapeHandle,
};
use std::sync::{
    Arc,
    mpsc::{Receiver, channel},
};

#[derive(Clone, PartialEq)]
pub(super) struct BodyKey {
    generation: u64,
    pick: Pick,
    shape: ShapeHandle,
    material: Option<(String, f64)>,
}
enum ReadError {
    Kernel(KernelError),
    Panicked,
}
/// 受け口は閉じた後も仕事の区切りまで残し、読み取りを文書の再計算と区別する。
pub(crate) struct BodyReadTask {
    key: BodyKey,
    source: DocumentData,
    control: Arc<JobControl>,
    receiver: Receiver<Result<Option<measure::BodyFacts>, ReadError>>,
    read_only: bool,
}
impl Drop for BodyReadTask {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

impl Workspace {
    pub(super) fn measure_body_request(
        &mut self,
        face: Option<FeatureId>,
    ) -> Option<measure::BodyFacts> {
        let key = face.and_then(|id| {
            let [Target::Face(pick)] = self.cur.measure.as_ref()?.picks.as_slice() else {
                return None;
            };
            let shape = self.cur.scene.key().iter().find(|(f, _)| *f == id)?.1;
            Some(BodyKey {
                generation: self.cur.scene.generation(),
                pick: pick.clone(),
                shape,
                material: kmb_session::body_material(
                    self.cur.work.doc().data(),
                    self.body_root(id),
                ),
            })
        });
        if let Some(task) = &self.cur.measure_job
            && Some(&task.key) != key.as_ref()
        {
            task.control.cancel();
        }
        let m = self.cur.measure.as_mut()?;
        if m.body_key != key {
            m.body_key = key;
            m.body = None;
            m.body_done = false;
        }
        m.body.clone()
    }

    /// 文書の変更で始める計算には、読み取り専用の扱いを持ち越さない。
    pub(crate) fn measure_document_changed(&mut self) {
        if let Some(task) = &mut self.cur.measure_job {
            task.read_only = false;
            task.control.cancel();
        }
    }
    pub(crate) fn only_measure_read_is_running(&self) -> bool {
        self.cur
            .measure_job
            .as_ref()
            .is_some_and(|task| task.read_only && task.source == *self.cur.work.doc().data())
    }
    pub(super) fn cancel_measure_read(&mut self) {
        if let Some(task) = &self.cur.measure_job {
            task.control.cancel();
        }
    }
    pub(super) fn measure_is_waiting(&self) -> bool {
        self.cur
            .measure
            .as_ref()
            .is_some_and(|m| m.body_key.is_some() && !m.body_done)
    }

    /// 派生データを受け取り、次の読み取りがあれば、文書の計算の空いた所で始める。
    pub(crate) fn measure_tick(&mut self, ctx: &egui::Context) {
        if self.cur.measure.is_none() {
            self.cancel_measure_read();
        }
        if !self.cur.work.is_recomputing()
            && let Some(task) = self.cur.measure_job.take()
        {
            let current = self.cur.measure.as_ref().and_then(|m| m.body_key.as_ref());
            if !task.control.cancelled()
                && current == Some(&task.key)
                && task.source == *self.cur.work.doc().data()
            {
                let result = task.receiver.try_recv().unwrap_or(Err(ReadError::Panicked));
                let m = self.cur.measure.as_mut().expect("同じ要求がある");
                m.body_done = true;
                match result {
                    Ok(body) => m.body = body,
                    Err(e) => {
                        let message = match e {
                            ReadError::Kernel(e) => e.to_string(),
                            ReadError::Panicked => {
                                crate::window_state::Words::of(ctx).t("measure-job-failed")
                            }
                        };
                        self.win.fail(Problem::new(ProblemKind::Compute, message));
                    }
                }
                self.measure_refresh();
            }
        }
        if !self.win.background || self.cur.work.is_recomputing() || self.cur.measure_job.is_some()
        {
            if self.measure_is_waiting() {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
            return;
        }
        let Some(key) = self
            .cur
            .measure
            .as_ref()
            .filter(|m| !m.body_done)
            .and_then(|m| m.body_key.clone())
        else {
            return;
        };
        if key.generation != self.cur.scene.generation()
            || !self
                .cur
                .scene
                .key()
                .contains(&(key.pick.feature, key.shape))
        {
            return;
        }
        let control = Arc::new(JobControl::new());
        control.progress(JobProgress::default());
        let (tx, receiver) = channel();
        let c = control.clone();
        let requested = key.clone();
        let read = Box::new(
            move |kernel: &dyn kmb_core::Kernel, bodies: &[(FeatureId, ShapeHandle)]| {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if !bodies.contains(&(requested.pick.feature, requested.shape)) {
                        return Ok(None);
                    }
                    measure::BodyFacts::of_cancellable(
                        kernel,
                        requested.shape,
                        requested.material,
                        || c.cancelled(),
                    )
                    .map_err(ReadError::Kernel)
                }))
                .unwrap_or(Err(ReadError::Panicked));
                let _ = tx.send(result);
            },
        );
        let after = self.scene_after(Some(read));
        let task = BodyReadTask {
            key,
            source: self.cur.work.doc().data().clone(),
            control,
            receiver,
            read_only: true,
        };
        let wake = self
            .win
            .repaint
            .clone()
            .map(|ctx| Box::new(move || ctx.request_repaint()) as Box<dyn FnOnce() + Send>);
        self.cur.measure_job = Some(task);
        self.cur.work.start_recompute(after, wake);
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}
