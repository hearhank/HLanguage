//! 协程调度器（ADR-0028：自 ir/mod.rs 拆分；M:N 模型，IR 版本）

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use super::g_id_ir::GIdIr;
use super::g_result_ir::GResultIr;
use super::g_state_ir::GStateIr;
use super::goroutine_ir::GoroutineIr;
use super::scheduler_inner_ir::SchedulerInnerIr;

/// 协程调度器（M:N 模型，IR 版本）
pub(crate) struct GoroutineSchedulerIr {
    inner: Arc<Mutex<SchedulerInnerIr>>,
    workers: Vec<thread::JoinHandle<()>>,
    started: bool,
    stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for GoroutineSchedulerIr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoroutineSchedulerIr")
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

impl GoroutineSchedulerIr {
    pub fn new() -> Self {
        let n = std::thread::available_parallelism()
            .map(|v| v.get())
            .unwrap_or(4);
        GoroutineSchedulerIr {
            inner: Arc::new(Mutex::new(SchedulerInnerIr {
                global_queue: VecDeque::new(),
                goroutines: HashMap::new(),
                next_gid: 1,
                num_workers: n,
            })),
            workers: Vec::new(),
            started: false,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 启动 worker 线程池
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let n = { self.inner.lock().unwrap().num_workers };
        for i in 0..n {
            let inner = self.inner.clone();
            let stop = self.stop.clone();
            let worker = thread::Builder::new()
                .name(format!("hc-worker-{i}"))
                .spawn(move || {
                    Self::worker_loop(inner, stop);
                })
                .expect("spawn scheduler worker");
            self.workers.push(worker);
        }
    }

    /// Worker 主循环：从全局队列取 G、执行任务、标记完成
    fn worker_loop(inner: Arc<Mutex<SchedulerInnerIr>>, stop: Arc<AtomicBool>) {
        while !stop.load(Ordering::Relaxed) {
            let gid = {
                let mut s = inner.lock().unwrap();
                s.global_queue.pop_front()
            };
            let Some(gid) = gid else {
                thread::sleep(std::time::Duration::from_millis(1));
                continue;
            };
            let task = {
                let mut s = inner.lock().unwrap();
                if let Some(g) = s.goroutines.get_mut(&gid) {
                    g.state = GStateIr::Running;
                    g.task.take()
                } else {
                    None
                }
            };
            if let Some(task) = task {
                task();
                let mut s = inner.lock().unwrap();
                if let Some(g) = s.goroutines.get_mut(&gid) {
                    g.state = GStateIr::Done;
                }
            }
        }
    }

    /// 提交一个协程到调度器
    pub fn submit(&self, name: String, task: Box<dyn FnOnce() + Send>) -> GIdIr {
        let mut s = self.inner.lock().unwrap();
        let id = s.next_gid;
        s.next_gid += 1;
        s.goroutines.insert(
            id,
            GoroutineIr {
                id,
                state: GStateIr::Runnable,
                name,
                task: Some(task),
                result: None,
            },
        );
        s.global_queue.push_back(id);
        id
    }

    /// 获取协程状态
    pub fn get_state(&self, id: GIdIr) -> Option<GStateIr> {
        self.inner
            .lock()
            .unwrap()
            .goroutines
            .get(&id)
            .map(|g| g.state)
    }

    /// 获取协程结果
    pub fn get_result(&self, id: GIdIr) -> Option<GResultIr> {
        self.inner
            .lock()
            .unwrap()
            .goroutines
            .get(&id)
            .and_then(|g| g.result.clone())
    }

    /// 设置协程结果
    pub fn set_result(&self, id: GIdIr, result: GResultIr) {
        let mut s = self.inner.lock().unwrap();
        if let Some(g) = s.goroutines.get_mut(&id) {
            g.result = Some(result);
        }
    }
}

impl Drop for GoroutineSchedulerIr {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let workers = std::mem::take(&mut self.workers);
        for w in workers {
            let _ = w.join();
        }
    }
}

impl Default for GoroutineSchedulerIr {
    fn default() -> Self {
        Self::new()
    }
}
