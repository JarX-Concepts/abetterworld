use log::{Level, Metadata, Record, SetLoggerError};

struct IOSLogger;

impl log::Log for IOSLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info // adjust as needed
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // Option 1: Just println
            println!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: IOSLogger = IOSLogger;

pub fn init_logger() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info))
}
