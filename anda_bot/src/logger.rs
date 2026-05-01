use chrono::{Local, NaiveDate};
use std::{
    collections::BTreeMap,
    fs::File,
    io,
    io::Write,
    path::{Path, PathBuf},
};
use structured_logger::{Builder, Writer, get_env_level};

pub const CLI_LOG_FILE_PREFIX: &str = "anda-cli";
pub const DAEMON_LOG_FILE_PREFIX: &str = "anda-daemon";

pub fn init_daily_json_logger(logs_dir: PathBuf, file_prefix: &'static str) -> io::Result<()> {
    Builder::with_level(&get_env_level().to_string())
        .with_target_writer("*", new_daily_json_writer(logs_dir, file_prefix)?)
        .init();
    Ok(())
}

pub fn current_daily_log_file_path(logs_dir: PathBuf, file_prefix: &str) -> PathBuf {
    logs_dir.join(daily_log_file_name(file_prefix, Local::now().date_naive()))
}

struct DailyJsonWriter {
    state: parking_lot::Mutex<DailyJsonWriterState>,
}

struct DailyJsonWriterState {
    logs_dir: PathBuf,
    file_prefix: &'static str,
    current_date: NaiveDate,
    file: File,
}

impl DailyJsonWriter {
    fn new(logs_dir: PathBuf, file_prefix: &'static str) -> io::Result<Self> {
        std::fs::create_dir_all(&logs_dir)?;

        let current_date = Local::now().date_naive();
        let file = open_daily_log_file(&logs_dir, file_prefix, current_date)?;

        Ok(Self {
            state: parking_lot::Mutex::new(DailyJsonWriterState {
                logs_dir,
                file_prefix,
                current_date,
                file,
            }),
        })
    }
}

impl Writer for DailyJsonWriter {
    fn write_log(&self, value: &BTreeMap<log::kv::Key, log::kv::Value>) -> Result<(), io::Error> {
        let mut buf = Vec::with_capacity(256);
        serde_json::to_writer(&mut buf, value).map_err(io::Error::from)?;
        buf.write_all(b"\n")?;

        let current_date = Local::now().date_naive();
        let mut state = self.state.lock();
        if state.current_date != current_date {
            state.file = open_daily_log_file(&state.logs_dir, state.file_prefix, current_date)?;
            state.current_date = current_date;
        }

        state.file.write_all(&buf)
    }
}

fn new_daily_json_writer(
    logs_dir: PathBuf,
    file_prefix: &'static str,
) -> io::Result<Box<dyn Writer>> {
    Ok(Box::new(DailyJsonWriter::new(logs_dir, file_prefix)?))
}

fn open_daily_log_file(logs_dir: &Path, file_prefix: &str, date: NaiveDate) -> io::Result<File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join(daily_log_file_name(file_prefix, date)))
}

fn daily_log_file_name(file_prefix: &str, date: NaiveDate) -> String {
    format!("{}-{}.log", file_prefix, date.format("%Y%m%d"))
}
