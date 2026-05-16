use crate::app::TickMsg;
use crate::sessions;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn spawn(tx: Sender<TickMsg>, gen_counter: Arc<AtomicU64>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let jobs_dir: PathBuf = dirs::home_dir().unwrap_or_default().join(".claude/jobs");
        loop {
            thread::sleep(Duration::from_secs(10));
            let generation = gen_counter.load(Ordering::SeqCst);
            let jobs = sessions::scan_jobs(&jobs_dir).unwrap_or_default();
            if tx
                .send(TickMsg::JobsRefreshed {
                    generation,
                    sessions: jobs,
                })
                .is_err()
            {
                break;
            }
        }
    })
}
