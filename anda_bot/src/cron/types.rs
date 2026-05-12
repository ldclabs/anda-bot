use anda_core::{AgentOutput, Principal, RequestMeta, ToolOutput};
use anda_db::schema::{
    AndaDBSchema, BoxError, FieldEntry, FieldKey, FieldType, Schema, SchemaError,
};
use chrono::{DateTime, Utc};
use cron::Schedule as CronExprSchedule;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr, time::Duration};

// Number.MAX_SAFE_INTEGER in JavaScript, used to represent "never" for disabled jobs
pub const DISABLED_JOB_NEXT_RUN: u64 = (1 << 53) - 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Schedule {
    Cron { expr: String, tz: Option<String> },
    At { at: u64 },
    Every { every: u64 },
}

impl Schedule {
    pub fn validate(&self, now_ms: u64) -> Result<(), BoxError> {
        match self {
            Schedule::Cron { expr, tz } => {
                let _ = schedule_next(expr, now_ms, tz)?;
                Ok(())
            }
            Schedule::At { at } => {
                if *at <= now_ms {
                    return Err("scheduled 'at' time must be in the future".into());
                }
                Ok(())
            }
            Schedule::Every { every } => {
                if *every == 0 {
                    return Err("every must be greater than 0".into());
                }
                Ok(())
            }
        }
    }

    /// Calculates the next run time based on the schedule, returning a unix timestamp in seconds.
    pub fn next_run(&self, from_ms: u64) -> u64 {
        match self {
            Schedule::Cron { expr, tz } => schedule_next(expr, from_ms, tz)
                .map(|ms| ms / 1000) // convert to seconds
                .unwrap_or(DISABLED_JOB_NEXT_RUN),
            Schedule::At { at } => {
                if at > &from_ms {
                    *at / 1000
                } else {
                    DISABLED_JOB_NEXT_RUN
                }
            }
            Schedule::Every { every } => (from_ms / 1000)
                .checked_add(*every)
                .unwrap_or(DISABLED_JOB_NEXT_RUN),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Shell,
    Agent,
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobKind::Shell => write!(f, "shell"),
            JobKind::Agent => write!(f, "agent"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleKind {
    Cron,
    At,
    Every,
    Once,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CronJobOrigin {
    pub caller: Option<String>,
    pub user: Option<String>,
    pub source: Option<String>,
    pub reply_target: Option<String>,
    pub thread: Option<String>,
    pub workspace: Option<String>,
    pub conversation_id: Option<u64>,
    pub external_user: Option<bool>,
}

impl CronJobOrigin {
    pub fn from_meta_with_caller(meta: &RequestMeta, caller: &Principal) -> Option<Self> {
        Self::from_meta_and_caller(meta, Some(caller))
    }

    fn from_meta_and_caller(meta: &RequestMeta, caller: Option<&Principal>) -> Option<Self> {
        let origin = Self {
            caller: caller.map(Principal::to_text),
            user: meta.user.as_deref().and_then(normalize_optional_name),
            source: meta
                .get_extra_as::<String>("source")
                .as_deref()
                .and_then(normalize_optional_name),
            reply_target: meta
                .get_extra_as::<String>("reply_target")
                .as_deref()
                .and_then(normalize_optional_name),
            thread: meta
                .get_extra_as::<String>("thread")
                .as_deref()
                .and_then(normalize_optional_name),
            workspace: meta
                .get_extra_as::<String>("workspace")
                .as_deref()
                .and_then(normalize_optional_name),
            conversation_id: meta
                .get_extra_as::<u64>("conversation")
                .filter(|conversation_id| *conversation_id > 0),
            external_user: meta.get_extra_as::<bool>("external_user"),
        };

        (!origin.is_empty()).then_some(origin)
    }

    pub fn caller_principal(&self) -> Option<Principal> {
        self.caller
            .as_deref()
            .and_then(|caller| Principal::from_text(caller).ok())
    }

    pub fn to_request_meta(&self, conversation_id: Option<u64>) -> RequestMeta {
        let mut extra = serde_json::Map::new();
        let conversation_id = conversation_id.or(self.conversation_id);
        if let Some(conversation_id) =
            conversation_id.filter(|conversation_id| *conversation_id > 0)
        {
            extra.insert("conversation".to_string(), conversation_id.into());
        }
        if let Some(source) = &self.source {
            extra.insert("source".to_string(), source.clone().into());
        }
        if let Some(reply_target) = &self.reply_target {
            extra.insert("reply_target".to_string(), reply_target.clone().into());
        }
        if let Some(thread) = &self.thread {
            extra.insert("thread".to_string(), thread.clone().into());
        }
        if let Some(workspace) = &self.workspace {
            extra.insert("workspace".to_string(), workspace.clone().into());
        }
        if let Some(external_user) = self.external_user {
            extra.insert("external_user".to_string(), external_user.into());
        }

        RequestMeta {
            user: self.user.clone(),
            extra,
            ..Default::default()
        }
    }

    fn is_empty(&self) -> bool {
        self.caller.is_none()
            && self.user.is_none()
            && self.source.is_none()
            && self.reply_target.is_none()
            && self.thread.is_none()
            && self.workspace.is_none()
            && self.conversation_id.is_none()
            && self.external_user.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, AndaDBSchema)]
pub struct CronJob {
    pub _id: u64,

    #[field_type = "Option<Map<Text, Json>>"]
    pub origin: Option<CronJobOrigin>,

    #[field_type = "Text"]
    pub job_kind: JobKind,
    pub job: String,

    #[field_type = "Text"]
    pub schedule_kind: ScheduleKind,
    pub schedule: String,
    pub tz: Option<String>,
    pub name: Option<String>,
    pub created_at: u64, // unix timestamp in milliseconds
    pub updated_at: u64, // unix timestamp in milliseconds
    pub next_run: u64,   // unix timestamp in seconds

    pub last_finished_at: Option<u64>,
    pub last_result: Option<String>,
    pub last_error: Option<String>,
    pub last_conversation_id: Option<u64>,
}

impl CronJob {
    pub fn schedule(&self) -> Result<Schedule, BoxError> {
        build_schedule(&self.schedule_kind, &self.schedule, self.tz.as_ref())
    }

    pub fn request_meta(&self) -> Option<RequestMeta> {
        let origin = self.origin.clone().unwrap_or_default();
        let conversation_id = self.last_conversation_id.or(origin.conversation_id);
        if origin.is_empty() && conversation_id.is_none() {
            return None;
        }
        let mut meta = origin.to_request_meta(conversation_id);
        meta.extra.insert(
            "cron_job_name".to_string(),
            self.name.clone().unwrap_or_default().into(),
        );
        meta.extra.insert(
            "cron_job_kind".to_string(),
            self.job_kind.to_string().into(),
        );
        meta.extra
            .insert("cron_job".to_string(), self.job.clone().into());

        Some(meta)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, AndaDBSchema)]
pub struct CronRun {
    pub _id: u64,
    pub job_id: u64,
    pub started_at: u64,  // unix timestamp in milliseconds
    pub finished_at: u64, // unix timestamp in milliseconds

    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateCronJobArgs {
    pub job_kind: JobKind,
    pub job: String,
    pub schedule_kind: ScheduleKind,
    pub schedule: String,
    pub name: Option<String>,
    pub tz: Option<String>,
}

impl CreateCronJobArgs {
    #[allow(unused)]
    pub fn into_cron_job(self, now_ms: u64) -> Result<CronJob, BoxError> {
        self.into_cron_job_with_origin(now_ms, None)
    }

    pub fn into_cron_job_with_origin(
        self,
        now_ms: u64,
        origin: Option<CronJobOrigin>,
    ) -> Result<CronJob, BoxError> {
        let schedule = build_schedule(&self.schedule_kind, &self.schedule, self.tz.as_ref())?;
        schedule.validate(now_ms)?;
        let next_run = schedule.next_run(now_ms);
        let (schedule_kind, schedule_str) =
            persisted_schedule(&self.schedule_kind, &self.schedule, &schedule)?;
        Ok(CronJob {
            _id: 0, // to be set by the store
            origin,
            job_kind: self.job_kind,
            job: self.job,
            schedule_kind,
            schedule: schedule_str,
            tz: self.tz,
            name: self.name,
            created_at: now_ms,
            updated_at: now_ms,
            next_run,
            last_finished_at: None,
            last_result: None,
            last_error: None,
            last_conversation_id: None,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct CronJobResult {
    pub conversation_id: Option<u64>,
    pub result: Option<String>,
    pub error: Option<String>,
}

impl From<BoxError> for CronJobResult {
    fn from(err: BoxError) -> Self {
        CronJobResult {
            conversation_id: None,
            result: None,
            error: Some(err.to_string()),
        }
    }
}

impl From<AgentOutput> for CronJobResult {
    fn from(output: AgentOutput) -> Self {
        CronJobResult {
            conversation_id: output.conversation,
            result: if output.failed_reason.is_none() {
                Some(output.content)
            } else {
                None
            },
            error: output.failed_reason,
        }
    }
}

impl<T> From<ToolOutput<T>> for CronJobResult
where
    T: Serialize,
{
    fn from(output: ToolOutput<T>) -> Self {
        match serde_json::to_string(&output.output) {
            Ok(result_str) => CronJobResult {
                conversation_id: None,
                result: Some(result_str),
                error: None,
            },
            Err(err) => CronJobResult {
                conversation_id: None,
                result: None,
                error: Some(format!("failed to serialize tool output: {err}")),
            },
        }
    }
}

fn persisted_schedule(
    requested_kind: &ScheduleKind,
    raw_value: &str,
    schedule: &Schedule,
) -> Result<(ScheduleKind, String), BoxError> {
    match requested_kind {
        ScheduleKind::Cron => Ok((ScheduleKind::Cron, raw_value.to_string())),
        ScheduleKind::At => Ok((ScheduleKind::At, raw_value.to_string())),
        ScheduleKind::Every => Ok((ScheduleKind::Every, raw_value.to_string())),
        ScheduleKind::Once => match schedule {
            Schedule::At { at } => Ok((ScheduleKind::At, unix_ms_to_rfc3339(*at)?)),
            _ => Err("once schedule must resolve to a single timestamp".into()),
        },
    }
}

fn schedule_next(expr: &str, from_ms: u64, tz: &Option<String>) -> Result<u64, BoxError> {
    let normalized = normalize_expression(expr)?;
    let from = DateTime::from_timestamp_millis(from_ms as i64).unwrap();
    let cron = CronExprSchedule::from_str(&normalized)
        .map_err(|err| format!("invalid cron expression '{expr}': {err}"))?;

    if let Some(tz_name) = tz {
        let timezone = chrono_tz::Tz::from_str(tz_name)
            .map_err(|err| format!("invalid IANA timezone '{tz_name}': {err}"))?;
        let localized_from = from.with_timezone(&timezone);
        let next_local = cron
            .after(&localized_from)
            .next()
            .ok_or_else(|| format!("no future occurrence for expression '{expr}'"))?;
        Ok(next_local.with_timezone(&Utc).timestamp_millis() as u64)
    } else {
        let local_from = from.with_timezone(&chrono::Local);
        let next_local = cron
            .after(&local_from)
            .next()
            .ok_or_else(|| format!("no future occurrence for expression '{expr}'"))?;
        Ok(next_local.with_timezone(&Utc).timestamp_millis() as u64)
    }
}

fn build_schedule(
    kind: &ScheduleKind,
    value: &str,
    tz: Option<&String>,
) -> Result<Schedule, BoxError> {
    match kind {
        ScheduleKind::Cron => Ok(Schedule::Cron {
            expr: normalize_required_text("schedule", value)?,
            tz: tz.and_then(|s| normalize_optional_name(s)),
        }),
        ScheduleKind::At => {
            if tz.is_some() {
                return Err("tz can only be used with cron schedules".into());
            }

            Ok(Schedule::At {
                at: datetime_to_unix_ms(
                    DateTime::parse_from_rfc3339(
                        normalize_required_text("schedule", value)?.as_str(),
                    )?
                    .with_timezone(&Utc),
                )?,
            })
        }
        ScheduleKind::Every => {
            if tz.is_some() {
                return Err("tz can only be used with cron schedules".into());
            }

            Ok(Schedule::Every {
                every: parse_delay(normalize_required_text("schedule", value)?.as_str())?.as_secs(),
            })
        }
        ScheduleKind::Once => {
            if tz.is_some() {
                return Err("tz can only be used with cron schedules".into());
            }

            let at =
                Utc::now() + parse_delay(normalize_required_text("schedule", value)?.as_str())?;
            Ok(Schedule::At {
                at: datetime_to_unix_ms(at)?,
            })
        }
    }
}

fn parse_delay(input: &str) -> Result<Duration, BoxError> {
    let input = input.trim();
    if input.is_empty() {
        return Err("delay must not be empty".into());
    }

    let split = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    let (num, unit) = input.split_at(split);
    let amount: u64 = num.parse()?;
    let unit = if unit.is_empty() { "s" } else { unit };

    match unit {
        "s" => Ok(Duration::from_secs(amount)),
        "m" => Ok(Duration::from_mins(amount)),
        "h" => Ok(Duration::from_hours(amount)),
        "d" => Ok(Duration::from_hours(amount * 24)),
        _ => Err(format!("unsupported delay unit '{unit}', use s/m/h/d").into()),
    }
}

fn normalize_optional_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn normalize_required_text(field: &str, value: &str) -> Result<String, BoxError> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field} must not be empty").into())
    } else {
        Ok(value.to_string())
    }
}

fn datetime_to_unix_ms(dt: DateTime<Utc>) -> Result<u64, BoxError> {
    u64::try_from(dt.timestamp_millis()).map_err(|_| "timestamp before unix epoch".into())
}

fn unix_ms_to_rfc3339(unix_ms: u64) -> Result<String, BoxError> {
    let unix_ms = i64::try_from(unix_ms).map_err(|_| "timestamp after i64 range")?;
    let dt = DateTime::from_timestamp_millis(unix_ms)
        .ok_or_else(|| "timestamp is outside chrono range".to_string())?;
    Ok(dt.to_rfc3339())
}

fn normalize_expression(expression: &str) -> Result<String, BoxError> {
    let expression = expression.trim();
    let field_count = expression.split_whitespace().count();

    match field_count {
        5 => {
            let mut fields: Vec<&str> = expression.split_whitespace().collect();
            let normalized_weekday = normalize_weekday_field(fields[4])?;
            fields[4] = &normalized_weekday;
            Ok(format!(
                "0 {} {} {} {} {}",
                fields[0], fields[1], fields[2], fields[3], fields[4]
            ))
        }
        6 | 7 => Ok(expression.to_string()),
        _ => Err(format!(
            "invalid cron expression '{expression}' (expected 5, 6, or 7 fields, got {field_count})"
        )
        .into()),
    }
}

fn normalize_weekday_field(field: &str) -> Result<String, BoxError> {
    if field == "*" || field == "?" {
        return Ok(field.to_string());
    }

    if field.chars().any(|c| c.is_ascii_alphabetic()) {
        return Ok(field.to_string());
    }

    let mut result_parts = Vec::new();
    for part in field.split(',') {
        let (range_part, step) = if let Some((range, step)) = part.split_once('/') {
            (range, Some(step))
        } else {
            (part, None)
        };

        let translated = if let Some((start_s, end_s)) = range_part.split_once('-') {
            let start: u8 = start_s
                .parse()
                .map_err(|err| format!("invalid weekday '{start_s}': {err}"))?;
            let end: u8 = end_s
                .parse()
                .map_err(|err| format!("invalid weekday '{end_s}': {err}"))?;
            format!(
                "{}-{}",
                translate_weekday_value(start)?,
                translate_weekday_value(end)?
            )
        } else if range_part == "*" {
            "*".to_string()
        } else {
            let value: u8 = range_part
                .parse()
                .map_err(|err| format!("invalid weekday '{range_part}': {err}"))?;
            translate_weekday_value(value)?.to_string()
        };

        if let Some(step) = step {
            result_parts.push(format!("{translated}/{step}"));
        } else {
            result_parts.push(translated);
        }
    }

    Ok(result_parts.join(","))
}

fn translate_weekday_value(val: u8) -> Result<u8, BoxError> {
    match val {
        0 | 7 => Ok(1),
        1..=6 => Ok(val + 1),
        _ => Err(format!("invalid weekday value {val}, expected 0-7").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_cron_args_deserializes_minimal_model_json() {
        let args: CreateCronJobArgs = serde_json::from_value(json!({
            "job_kind": "shell",
            "job": "echo hello",
            "schedule_kind": "every",
            "schedule": "60000"
        }))
        .unwrap();

        assert!(matches!(args.job_kind, JobKind::Shell));
        assert_eq!(args.job, "echo hello");
        assert!(matches!(args.schedule_kind, ScheduleKind::Every));
        assert_eq!(args.schedule, "60000");
        assert_eq!(args.name, None);
        assert_eq!(args.tz, None);
    }

    #[test]
    fn create_cron_args_deserializes_model_json_with_optional_fields() {
        let args: CreateCronJobArgs = serde_json::from_value(json!({
            "job_kind": "agent",
            "job": "Send the daily summary to me",
            "schedule_kind": "cron",
            "schedule": "0 9 * * 1-5",
            "name": "daily-summary",
            "tz": "Asia/Shanghai"
        }))
        .unwrap();

        assert!(matches!(args.job_kind, JobKind::Agent));
        assert_eq!(args.job, "Send the daily summary to me");
        assert!(matches!(args.schedule_kind, ScheduleKind::Cron));
        assert_eq!(args.schedule, "0 9 * * 1-5");
        assert_eq!(args.name, Some("daily-summary".to_string()));
        assert_eq!(args.tz, Some("Asia/Shanghai".to_string()));
    }

    #[test]
    fn cron_origin_round_trips_request_meta() {
        let caller = Principal::from_text("aaaaa-aa").unwrap();
        let mut extra = serde_json::Map::new();
        extra.insert("source".to_string(), "wechat:daily".into());
        extra.insert("reply_target".to_string(), "alice".into());
        extra.insert("thread".to_string(), "morning".into());
        extra.insert("workspace".to_string(), "/tmp/anda/wechat".into());
        extra.insert("conversation".to_string(), 42.into());
        extra.insert("external_user".to_string(), true.into());
        let meta = RequestMeta {
            user: Some("alice".to_string()),
            extra,
            ..Default::default()
        };

        let origin = CronJobOrigin::from_meta_with_caller(&meta, &caller).unwrap();
        assert_eq!(origin.caller, Some(caller.to_text()));
        assert_eq!(origin.caller_principal(), Some(caller));
        assert_eq!(origin.user, Some("alice".to_string()));
        assert_eq!(origin.source, Some("wechat:daily".to_string()));
        assert_eq!(origin.reply_target, Some("alice".to_string()));
        assert_eq!(origin.thread, Some("morning".to_string()));
        assert_eq!(origin.workspace, Some("/tmp/anda/wechat".to_string()));
        assert_eq!(origin.conversation_id, Some(42));
        assert_eq!(origin.external_user, Some(true));

        let meta = origin.to_request_meta(Some(77));
        assert_eq!(meta.user, Some("alice".to_string()));
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("wechat:daily".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("reply_target"),
            Some("alice".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("thread"),
            Some("morning".to_string())
        );
        assert_eq!(meta.get_extra_as::<u64>("conversation"), Some(77));
        assert_eq!(meta.get_extra_as::<bool>("external_user"), Some(true));
    }

    #[test]
    fn cron_job_request_meta_prefers_last_conversation() {
        let now_ms = Utc::now().timestamp_millis() as u64;
        let origin = CronJobOrigin {
            source: Some("wechat:daily".to_string()),
            reply_target: Some("alice".to_string()),
            conversation_id: Some(42),
            ..Default::default()
        };
        let mut job = CreateCronJobArgs {
            job_kind: JobKind::Agent,
            job: "Send the daily summary to me".to_string(),
            schedule_kind: ScheduleKind::Every,
            schedule: "1h".to_string(),
            name: Some("daily-summary".to_string()),
            tz: None,
        }
        .into_cron_job_with_origin(now_ms, Some(origin))
        .unwrap();
        job.last_conversation_id = Some(88);

        let meta = job.request_meta().unwrap();
        assert_eq!(meta.get_extra_as::<u64>("conversation"), Some(88));
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("wechat:daily".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("reply_target"),
            Some("alice".to_string())
        );
    }

    #[test]
    fn build_schedule_cron_preserves_expr_and_tz() {
        let schedule = build_schedule(
            &ScheduleKind::Cron,
            "0 9 * * 1-5",
            Some(&"Asia/Shanghai".to_string()),
        )
        .unwrap();

        assert_eq!(
            schedule,
            Schedule::Cron {
                expr: "0 9 * * 1-5".to_string(),
                tz: Some("Asia/Shanghai".to_string()),
            }
        );
    }

    #[test]
    fn build_schedule_at_parses_rfc3339_timestamp() {
        let schedule = build_schedule(&ScheduleKind::At, "2026-04-22T15:04:05Z", None).unwrap();

        assert_eq!(
            schedule,
            Schedule::At {
                at: 1_776_870_245_000,
            }
        );
    }

    #[test]
    fn build_schedule_every_defaults_to_seconds() {
        let schedule = build_schedule(&ScheduleKind::Every, "60", None).unwrap();

        assert_eq!(schedule, Schedule::Every { every: 60 });
    }

    #[test]
    fn build_schedule_every_parses_duration_units() {
        let schedule = build_schedule(&ScheduleKind::Every, "5m", None).unwrap();

        assert_eq!(schedule, Schedule::Every { every: 300 });
    }

    #[test]
    fn build_schedule_once_returns_future_timestamp() {
        let before = Utc::now().timestamp_millis() as u64;
        let schedule = build_schedule(&ScheduleKind::Once, "30m", None).unwrap();
        let after = Utc::now().timestamp_millis() as u64;

        let Schedule::At { at } = schedule else {
            panic!("expected once schedule to map to Schedule::At");
        };

        let min_expected = before + 30 * 60 * 1000;
        let max_expected = after + 30 * 60 * 1000;
        assert!(at >= min_expected, "expected {at} >= {min_expected}");
        assert!(at <= max_expected, "expected {at} <= {max_expected}");
    }

    #[test]
    fn create_once_cron_job_keeps_next_run_in_sync_with_persisted_at_schedule() {
        let now_ms = Utc::now().timestamp_millis() as u64;
        let job = CreateCronJobArgs {
            job_kind: JobKind::Agent,
            job: "Send the daily summary to me".to_string(),
            schedule_kind: ScheduleKind::Once,
            schedule: "30m".to_string(),
            name: Some("daily-summary".to_string()),
            tz: None,
        }
        .into_cron_job(now_ms)
        .unwrap();

        assert!(matches!(job.schedule_kind, ScheduleKind::At));

        let stored_at = datetime_to_unix_ms(
            DateTime::parse_from_rfc3339(&job.schedule)
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();

        assert_eq!(job.next_run, stored_at / 1000);
    }

    #[test]
    fn parse_delay_supports_units_and_defaults_to_seconds() {
        assert_eq!(parse_delay("15").unwrap(), Duration::from_secs(15));
        assert_eq!(parse_delay("15s").unwrap(), Duration::from_secs(15));
        assert_eq!(parse_delay("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_delay("3h").unwrap(), Duration::from_secs(10_800));
        assert_eq!(parse_delay("1d").unwrap(), Duration::from_secs(86_400));
    }

    #[test]
    fn build_schedule_rejects_tz_for_non_cron() {
        let err = build_schedule(
            &ScheduleKind::At,
            "2026-04-22T15:04:05Z",
            Some(&"Asia/Shanghai".to_string()),
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "tz can only be used with cron schedules");
    }
}
