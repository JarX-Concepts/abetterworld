use android_log_sys::{LogPriority, __android_log_print};
use log::{Level, Metadata, Record};

struct AndroidLogger;

impl log::Log for AndroidLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let prio = match record.level() {
            Level::Error => LogPriority::ERROR,
            Level::Warn => LogPriority::WARN,
            Level::Info => LogPriority::INFO,
            Level::Debug => LogPriority::DEBUG,
            Level::Trace => LogPriority::VERBOSE,
        };

        // SAFETY: format! produces a valid C string when we pass via CString.
        use std::ffi::CString;
        let tag = CString::new("abetterworld").unwrap();
        let msg = CString::new(format!("{}", record.args())).unwrap();
        unsafe { __android_log_print(prio as i32, tag.as_ptr(), msg.as_ptr()) };
    }

    fn flush(&self) {}
}

static LOGGER: AndroidLogger = AndroidLogger;

pub fn init_logger() -> Result<(), log::SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info))
}
