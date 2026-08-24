//! 通道操作（E4：M:N 协程通信——chan<T>）
//!
//! Task 1 实现：chan.init(alloc[, cap]) 构造 + send/recv/try_send/try_recv/close 方法。
//! 通道使用 Mutex<ChanInner> + Condvar 实现阻塞式 send/recv，非阻塞操作返回 bool/Opt。
//! 阻塞操作会阻塞当前 OS 线程（worker 线程），其他 worker 线程仍可继续运行。

use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::value::{ChanInner, ChanState};

impl Interp {
    /// 通道方法分派（chan<T> 的 send/recv/try_send/try_recv/close）
    pub(crate) fn call_chan_method(
        &mut self,
        ch: &Arc<ChanState>,
        field: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<Option<Value>> {
        match field {
            "send" => {
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                let mut inner = ch.inner.lock().unwrap();
                if inner.closed {
                    return Ok(Some(self.err_val("Closed")));
                }
                // Wait for buffer space (unbuffered: wait for receiver)
                while inner.queue.len() >= ch.capacity && !inner.closed {
                    inner = ch.send_cond.wait(inner).unwrap();
                }
                if inner.closed {
                    return Ok(Some(self.err_val("Closed")));
                }
                inner.queue.push_back(v);
                ch.recv_cond.notify_one();
                Ok(Some(Value::Void))
            }
            "recv" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let mut inner = ch.inner.lock().unwrap();
                while inner.queue.is_empty() && !inner.closed {
                    inner = ch.recv_cond.wait(inner).unwrap();
                }
                if inner.queue.is_empty() && inner.closed {
                    return Ok(Some(self.err_val("Closed")));
                }
                let v = inner.queue.pop_front().unwrap();
                ch.send_cond.notify_one();
                Ok(Some(v))
            }
            "try_send" => {
                if args.len() != 1 {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let v = self.eval(&args[0])?;
                let mut inner = ch.inner.lock().unwrap();
                if inner.closed || inner.queue.len() >= ch.capacity {
                    return Ok(Some(Value::Bool(false)));
                }
                inner.queue.push_back(v);
                ch.recv_cond.notify_one();
                Ok(Some(Value::Bool(true)))
            }
            "try_recv" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let mut inner = ch.inner.lock().unwrap();
                if let Some(v) = inner.queue.pop_front() {
                    ch.send_cond.notify_one();
                    Ok(Some(Value::Opt(Some(Rc::new(v)))))
                } else {
                    Ok(Some(Value::Opt(None)))
                }
            }
            "close" => {
                if !args.is_empty() {
                    return Err(RtError::new("ArityMismatch", Some(span.clone())));
                }
                let mut inner = ch.inner.lock().unwrap();
                inner.closed = true;
                ch.send_cond.notify_all();
                ch.recv_cond.notify_all();
                Ok(Some(Value::Void))
            }
            _ => Ok(None),
        }
    }
}
