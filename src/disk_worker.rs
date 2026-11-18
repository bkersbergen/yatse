use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

type Job = Box<dyn FnOnce() + Send>;

/// One ordered queue prevents overlapping writes to settings and Steam metadata.
pub struct DiskWorker {
    sender: Option<Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl DiskWorker {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<Job>();
        let thread = thread::Builder::new().name("yatse-disk".into()).spawn(move || {
            for job in receiver { job(); }
        }).expect("Could not start disk worker");
        Self { sender: Some(sender), thread: Some(thread) }
    }

    pub fn submit<T: Send + 'static>(&self, work: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
        let (sender, receiver) = mpsc::channel();
        self.sender.as_ref().unwrap().send(Box::new(move || {
            let _ = sender.send(work());
        })).expect("Disk worker stopped");
        receiver
    }
}

impl Drop for DiskWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_work_does_not_block_caller_and_runs_in_order() {
        let worker = DiskWorker::new();
        let caller = thread::current().id();
        let (release, wait) = mpsc::channel();
        let (started, started_rx) = mpsc::channel();
        let first = worker.submit(move || {
            started.send(thread::current().id()).unwrap();
            wait.recv().unwrap();
            1
        });
        assert_ne!(started_rx.recv().unwrap(), caller);
        let second = worker.submit(|| 2);
        assert!(matches!(second.try_recv(), Err(mpsc::TryRecvError::Empty)));
        release.send(()).unwrap();
        assert_eq!(first.recv().unwrap(), 1);
        assert_eq!(second.recv().unwrap(), 2);
    }
}
