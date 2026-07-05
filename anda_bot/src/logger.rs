use chrono::{Local, NaiveDate};
use std::{
    collections::BTreeMap,
    fs::File,
    io,
    io::Write,
    path::{Path, PathBuf},
};
use structured_logger::{Builder, Writer};

pub const CLI_LOG_FILE_PREFIX: &str = "anda-cli";
pub const DAEMON_LOG_FILE_PREFIX: &str = "anda-daemon";
// Daily log files older than this are removed at logger startup.
const LOG_RETENTION_DAYS: u64 = 30;

pub fn init_daily_json_logger(
    level: &str,
    logs_dir: PathBuf,
    file_prefix: &'static str,
) -> io::Result<()> {
    Builder::with_level(level)
        .with_target_writer("*", new_daily_json_writer(logs_dir.clone(), file_prefix)?)
        .init();
    prune_old_daily_logs(&logs_dir, LOG_RETENTION_DAYS);
    Ok(())
}

/// Best-effort removal of daily `.log` files older than `keep_days`, based on
/// the `-YYYYMMDD` date embedded in the file name. Files whose names do not
/// parse are left alone.
fn prune_old_daily_logs(logs_dir: &Path, keep_days: u64) {
    let today = Local::now().date_naive();
    let Some(cutoff) = today.checked_sub_days(chrono::Days::new(keep_days)) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(logs_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(date) = daily_log_file_date(name) else {
            continue;
        };
        if date < cutoff
            && let Err(err) = std::fs::remove_file(entry.path())
        {
            log::warn!("failed to prune old log file {name}: {err}");
        }
    }
}

fn daily_log_file_date(file_name: &str) -> Option<NaiveDate> {
    let stem = file_name.strip_suffix(".log")?;
    let (_, date) = stem.rsplit_once('-')?;
    NaiveDate::parse_from_str(date, "%Y%m%d").ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_log_file_name_formats_date_as_yyyymmdd() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();

        assert_eq!(
            daily_log_file_name("anda-cli", date),
            "anda-cli-20260601.log"
        );
    }

    #[test]
    fn current_daily_log_file_path_joins_logs_dir_and_prefix() {
        let path = current_daily_log_file_path(PathBuf::from("/tmp/anda/logs"), "anda-daemon");
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap();

        assert_eq!(path.parent(), Some(Path::new("/tmp/anda/logs")));
        assert!(file_name.starts_with("anda-daemon-"));
        assert!(file_name.ends_with(".log"));
    }

    #[test]
    fn daily_json_writer_creates_log_directory_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let date = Local::now().date_naive();
        let expected = dir.path().join(daily_log_file_name("anda-test", date));

        let _writer = DailyJsonWriter::new(dir.path().join("nested"), "anda-test").unwrap();

        assert!(dir.path().join("nested").is_dir());
        assert!(
            dir.path()
                .join("nested")
                .join(expected.file_name().unwrap())
                .is_file()
        );
    }

    #[test]
    fn daily_json_writer_appends_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let writer = DailyJsonWriter::new(dir.path().to_path_buf(), "anda-test").unwrap();

        let key = log::kv::Key::from_str("msg");
        let value = log::kv::Value::from("daemon started");
        let mut entry = BTreeMap::new();
        entry.insert(key, value);
        writer.write_log(&entry).unwrap();

        let date = Local::now().date_naive();
        let log_path = dir.path().join(daily_log_file_name("anda-test", date));
        let content = std::fs::read_to_string(log_path).unwrap();
        assert_eq!(content, "{\"msg\":\"daemon started\"}\n");
    }

    #[test]
    fn daily_log_file_date_parses_only_dated_log_names() {
        assert_eq!(
            daily_log_file_date("anda-daemon-20260601.log"),
            NaiveDate::from_ymd_opt(2026, 6, 1)
        );
        assert_eq!(daily_log_file_date("anda-daemon.log"), None);
        assert_eq!(daily_log_file_date("random.txt"), None);
    }

    #[test]
    fn prune_old_daily_logs_removes_only_expired_files() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("anda-test-20200101.log");
        let recent = dir
            .path()
            .join(daily_log_file_name("anda-test", Local::now().date_naive()));
        let unrelated = dir.path().join("notes.txt");
        std::fs::write(&old, "old").unwrap();
        std::fs::write(&recent, "recent").unwrap();
        std::fs::write(&unrelated, "keep").unwrap();

        prune_old_daily_logs(dir.path(), 30);

        assert!(!old.exists());
        assert!(recent.exists());
        assert!(unrelated.exists());
    }

    #[test]
    fn init_daily_json_logger_installs_global_writer() {
        let dir = tempfile::tempdir().unwrap();

        // The global logger can only be installed once per process; later
        // tests in this binary must tolerate it already being set.
        init_daily_json_logger("info", dir.path().to_path_buf(), "anda-test").unwrap();
        log::info!(target: "logger-test", "hello from test");
        log::logger().flush();

        let date = Local::now().date_naive();
        let log_path = dir.path().join(daily_log_file_name("anda-test", date));
        assert!(log_path.is_file());
    }
}
