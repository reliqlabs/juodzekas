use log::{Log, Metadata, Record};
use std::sync::{Arc, Mutex};

pub struct TuiLogger {
    log_buffer: Arc<Mutex<Vec<String>>>,
}

impl TuiLogger {
    pub fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
        let log_buffer = Arc::new(Mutex::new(Vec::new()));
        (
            TuiLogger {
                log_buffer: log_buffer.clone(),
            },
            log_buffer,
        )
    }
}

impl Log for TuiLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let msg = format!("{}", record.args());
            if let Ok(mut buffer) = self.log_buffer.lock() {
                buffer.push(msg);
                // Keep only last 100 messages to prevent memory issues
                if buffer.len() > 100 {
                    buffer.remove(0);
                }
            }
        }
    }

    fn flush(&self) {}
}
