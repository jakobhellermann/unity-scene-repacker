pub struct ParisLogger;
impl log::Log for ParisLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            match record.level() {
                log::Level::Error => paris::error!("{}", record.args()),
                log::Level::Warn => paris::warn!("{}", record.args()),
                log::Level::Info => paris::info!("{}", record.args()),
                log::Level::Debug => paris::info!("{}", record.args()),
                log::Level::Trace => paris::info!("{}", record.args()),
            }
        }
    }

    fn flush(&self) {}
}

pub fn install() {
    let _ = log::set_logger(&ParisLogger);
    log::set_max_level(log::LevelFilter::Info);
}
