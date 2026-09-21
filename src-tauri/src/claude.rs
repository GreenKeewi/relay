use serde::{
    de::{IgnoredAny, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use std::{
    env,
    ffi::{OsStr, OsString},
    fmt,
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const SOURCE: &str = "claude-code";
const RECENT_ACTIVITY_WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_RECORD_BYTES: usize = 4 * 1024 * 1024;
const LIVE_USAGE_SOURCE: &str = "claude-statusline";
const ACCOUNT_USAGE_SOURCE: &str = "claude-account-api";
const CCSTATUSLINE_USAGE_SOURCE: &str = "ccstatusline-cache";
const CLAUDE_USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const DIRECT_USAGE_RETRY_DELAY: Duration = Duration::from_secs(60);
// Matches ccstatusline's CACHE_MAX_AGE. Older snapshots remain useful only
// when clearly labeled stale; they must not be presented as current quota.
const CCSTATUSLINE_CACHE_MAX_AGE: Duration = Duration::from_secs(180);
const MAX_USAGE_CACHE_BYTES: u64 = 64 * 1024;
const MAX_CREDENTIAL_BYTES: u64 = 1024 * 1024;
const MAX_STATUSLINE_INPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_STATUSLINE_COMMAND: &str = "npx -y ccstatusline@latest";
const RELAY_STATUSLINE_BRIDGE_FILENAME: &str = "statusline-bridge.cjs";
const RELAY_STATUSLINE_CONFIG_FILENAME: &str = "statusline-upstream.json";
const RELAY_STATUSLINE_BACKUP_FILENAME: &str = "settings.pre-relay-statusline.json";

const RELAY_STATUSLINE_BRIDGE_SOURCE: &str = r#"'use strict';

const childProcess = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const chunks = [];
process.stdin.on('data', (chunk) => chunks.push(Buffer.from(chunk)));
process.stdin.on('end', () => {
  const input = Buffer.concat(chunks);

  try {
    const payload = JSON.parse(input.toString('utf8'));
    const fiveHour = payload?.rate_limits?.five_hour;
    const sevenDay = payload?.rate_limits?.seven_day;
    const percent = (value) => Number.isFinite(value) && value >= 0 && value <= 100
      ? value
      : null;
    const reset = (value) => Number.isFinite(value) && value > 0
      ? Math.round(value * 1000)
      : null;
    const snapshot = {
      capturedAtMs: Date.now(),
      sessionPercent: percent(fiveHour?.used_percentage),
      sessionResetAtMs: reset(fiveHour?.resets_at),
      weeklyPercent: percent(sevenDay?.used_percentage),
      weeklyResetAtMs: reset(sevenDay?.resets_at),
    };

    if (Object.values(snapshot).slice(1).some((value) => value !== null)) {
      const snapshotDir = path.join(os.homedir(), '.cache', 'relay');
      const snapshotPath = path.join(snapshotDir, 'claude-usage.json');
      const temporaryPath = `${snapshotPath}.${process.pid}.tmp`;
      fs.mkdirSync(snapshotDir, { recursive: true });
      fs.writeFileSync(temporaryPath, JSON.stringify(snapshot), { encoding: 'utf8', mode: 0o600 });
      try {
        fs.renameSync(temporaryPath, snapshotPath);
      } catch {
        fs.copyFileSync(temporaryPath, snapshotPath);
        fs.unlinkSync(temporaryPath);
      }
    }
  } catch {
    // Relay never blocks or changes the user's existing status line.
  }

  let upstreamCommand = '';
  try {
    const configPath = path.join(__dirname, 'statusline-upstream.json');
    const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
    if (typeof config.upstreamCommand === 'string') {
      upstreamCommand = config.upstreamCommand.trim();
    }
  } catch {
    // A missing upstream leaves the status line blank, matching command failure.
  }

  if (!upstreamCommand) return;

  const child = childProcess.spawn(upstreamCommand, {
    shell: true,
    windowsHide: true,
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  child.stdout.pipe(process.stdout);
  child.stderr.pipe(process.stderr);
  child.on('error', () => { process.exitCode = 1; });
  child.on('close', (code) => { process.exitCode = code ?? 0; });
  child.stdin.end(input);
});
"#;

/// The complete, privacy-preserving result of a local Claude Code scan.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDiscovery {
    pub detection: ClaudeDetection,
    pub sessions: Vec<ClaudeSessionSummary>,
}

/// Installation and scan facts Relay can show without exposing conversation data.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDetection {
    pub source: &'static str,
    pub installed: bool,
    pub cli_available: bool,
    pub projects_directory: PathBuf,
    pub projects_directory_exists: bool,
    pub session_files_seen: usize,
    pub sessions_discovered: usize,
    pub unreadable_files: usize,
    pub skipped_nested_directories: usize,
}

/// A deliberately small projection of a Claude JSONL transcript.
///
/// Prompt text, assistant text, tool inputs, and tool outputs are never fields on
/// this type and are never retained by the parser.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSessionSummary {
    pub id: String,
    pub source: &'static str,
    pub project: String,
    pub cwd: PathBuf,
    pub repository: Option<PathBuf>,
    pub branch: Option<String>,
    pub title: String,
    pub session_name: Option<String>,
    pub agent_name: Option<String>,
    pub safe_activity: Option<String>,
    pub last_activity_ms: u64,
    pub state: ClaudeSessionState,
    pub resume_available: bool,
}

/// This is intentionally conservative. Claude's transcript format does not
/// expose a trustworthy completed/review/failed lifecycle state.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeSessionState {
    Recent,
    Idle,
}

/// A privacy-preserving projection of ccstatusline's structured usage cache.
///
/// The cache also contains a token fingerprint used by ccstatusline for account
/// isolation. Relay intentionally neither deserializes nor returns that field.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsage {
    pub source: &'static str,
    pub status: ClaudeUsageStatus,
    pub reason: Option<ClaudeUsageReason>,
    pub session_percent: Option<f64>,
    pub session_reset_at_ms: Option<u64>,
    pub weekly_percent: Option<f64>,
    pub weekly_reset_at_ms: Option<u64>,
    pub updated_at_ms: Option<u64>,
    pub age_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayStatuslineBridgeConfig {
    schema_version: u8,
    upstream_command: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeUsageStatus {
    Available,
    Stale,
    Unavailable,
    Error,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeUsageReason {
    LiveSnapshotNotFound,
    CacheNotFound,
    CacheUnreadable,
    CacheTooLarge,
    InvalidCache,
    NoUsageData,
    Timeout,
    RateLimited,
    ApiError,
    ParseError,
    NoCredentials,
    AuthenticationExpired,
    UnknownUpstreamError,
}

/// Only the allow-listed metadata required for discovery is deserialized.
/// Unknown fields (including `message`, `content`, and `toolUseResult`) are
/// discarded by serde and can never leak into a summary.
#[derive(Debug, Deserialize)]
struct ClaudeRecordMetadata {
    #[serde(default, rename = "type")]
    record_type: Option<String>,
    #[serde(default, rename = "sessionId", alias = "session_id")]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default, rename = "gitBranch")]
    git_branch: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default, rename = "aiTitle")]
    ai_title: Option<String>,
    #[serde(default, rename = "agentName")]
    agent_name: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    message: Option<ClaudeMessageMetadata>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessageMetadata {
    #[serde(default, deserialize_with = "deserialize_tool_blocks")]
    content: Vec<ClaudeContentBlockMetadata>,
}

#[derive(Debug, Deserialize)]
struct ClaudeContentBlockMetadata {
    #[serde(default, rename = "type")]
    block_type: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

fn deserialize_tool_blocks<'de, D>(
    deserializer: D,
) -> Result<Vec<ClaudeContentBlockMetadata>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ToolBlockVisitor;

    impl<'de> Visitor<'de> for ToolBlockVisitor {
        type Value = Vec<ClaudeContentBlockMetadata>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a Claude message content string, object, or block array")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut blocks = Vec::new();
            while let Some(block) = sequence.next_element::<ClaudeContentBlockMetadata>()? {
                blocks.push(block);
            }
            Ok(blocks)
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(Vec::new())
        }
    }

    deserializer.deserialize_any(ToolBlockVisitor)
}

/// The allow-listed portion of `~/.cache/ccstatusline/usage.json`.
/// Unknown fields are discarded, most importantly `tokenHash`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CcstatuslineUsageCache {
    #[serde(default)]
    session_usage: Option<f64>,
    #[serde(default)]
    session_reset_at: Option<String>,
    #[serde(default)]
    weekly_usage: Option<f64>,
    #[serde(default)]
    weekly_reset_at: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Claude Code's documented, metadata-only rate-limit section from statusline
/// stdin. Every other statusline field is discarded by serde.
#[derive(Debug, Deserialize)]
struct ClaudeStatuslineUsageInput {
    #[serde(default)]
    rate_limits: Option<ClaudeStatuslineRateLimits>,
}

#[derive(Debug, Deserialize)]
struct ClaudeStatuslineRateLimits {
    #[serde(default)]
    five_hour: Option<ClaudeStatuslineRateLimitWindow>,
    #[serde(default)]
    seven_day: Option<ClaudeStatuslineRateLimitWindow>,
}

#[derive(Debug, Deserialize)]
struct ClaudeStatuslineRateLimitWindow {
    #[serde(default)]
    used_percentage: Option<f64>,
    #[serde(default)]
    resets_at: Option<f64>,
}

/// Relay-owned snapshot written by the lightweight statusline bridge. This is
/// intentionally much narrower than Claude Code's complete statusline payload.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayClaudeUsageSnapshot {
    #[serde(default)]
    source: RelayUsageSnapshotSource,
    captured_at_ms: u64,
    session_percent: Option<f64>,
    session_reset_at_ms: Option<u64>,
    weekly_percent: Option<f64>,
    weekly_reset_at_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RelayUsageSnapshotSource {
    #[default]
    Statusline,
    AccountApi,
}

impl RelayUsageSnapshotSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Statusline => LIVE_USAGE_SOURCE,
            Self::AccountApi => ACCOUNT_USAGE_SOURCE,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCredentialsFile {
    claude_ai_oauth: Option<ClaudeOauthCredentials>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOauthCredentials {
    access_token: String,
    expires_at: u64,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsageApiResponse {
    #[serde(default)]
    five_hour: Option<ClaudeUsageApiWindow>,
    #[serde(default)]
    seven_day: Option<ClaudeUsageApiWindow>,
    #[serde(default)]
    limits: Vec<ClaudeUsageApiLimit>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsageApiWindow {
    #[serde(default)]
    utilization: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsageApiLimit {
    kind: String,
    #[serde(default)]
    utilization: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
}

struct ParsedSession {
    id: String,
    cwd: PathBuf,
    branch: Option<String>,
    session_name: Option<String>,
    agent_name: Option<String>,
    safe_activity: Option<String>,
    last_activity: SystemTime,
}

#[derive(Default)]
struct DirectUsageRefreshState {
    last_attempt: Option<SystemTime>,
    last_result: Option<ClaudeUsage>,
}

static DIRECT_USAGE_REFRESH_STATE: OnceLock<Mutex<DirectUsageRefreshState>> = OnceLock::new();

/// Discover Claude Code sessions from the current user's standard data folder.
/// A missing installation is a valid empty result, not an error.
pub fn discover_claude_sessions() -> ClaudeDiscovery {
    let projects_directory = claude_projects_directory();
    discover_claude_sessions_in(
        &projects_directory,
        SystemTime::now(),
        claude_cli_available(),
    )
}

/// Resume a discovered Claude Code session in Windows Terminal.
///
/// The session identifier is restricted to canonical UUID syntax, the working
/// directory must resolve to an existing directory, and all process arguments
/// are passed as separate arguments. No project path or other arbitrary text is
/// interpolated into shell source.
pub fn resume_claude_session(session_id: &str, cwd: &str) -> Result<(), String> {
    validate_session_id(session_id)?;
    let cwd = validate_working_directory(cwd)?;
    spawn_resume_terminal(session_id, &cwd)
}

/// Read normalized Claude plan usage, preferring Claude's documented statusline
/// fields and refreshing from Anthropic only when local sources are not current.
pub fn read_claude_usage() -> ClaudeUsage {
    let cache_root = user_home_directory().join(".cache");
    let snapshot_path = cache_root.join("relay").join("claude-usage.json");
    let ccstatusline_cache_path = cache_root.join("ccstatusline").join("usage.json");
    let now = SystemTime::now();
    let local = read_claude_usage_from_sources(&snapshot_path, &ccstatusline_cache_path, now);
    if local.status == ClaudeUsageStatus::Available {
        return local;
    }

    let refresh_state =
        DIRECT_USAGE_REFRESH_STATE.get_or_init(|| Mutex::new(DirectUsageRefreshState::default()));
    if let Ok(mut state) = refresh_state.lock() {
        let retry_due = state
            .last_attempt
            .and_then(|attempt| now.duration_since(attempt).ok())
            .map(|elapsed| elapsed >= DIRECT_USAGE_RETRY_DELAY)
            .unwrap_or(true);
        if !retry_due {
            return state.last_result.clone().unwrap_or(local);
        }
        state.last_attempt = Some(now);
    }

    let credentials_path = claude_config_directory().join(".credentials.json");
    let refreshed = fetch_claude_usage_from_endpoint(
        &credentials_path,
        &snapshot_path,
        CLAUDE_USAGE_ENDPOINT,
        now,
    );
    if let Ok(mut state) = refresh_state.lock() {
        state.last_result = Some(refreshed.clone());
    }
    refreshed
}

/// Install Relay's local statusline bridge while preserving the user's
/// existing statusline command and every unrelated Claude setting.
///
/// The generated bridge forwards stdin to the previous command unchanged and
/// stores only the four documented rate-limit values Relay displays. It never
/// persists prompts, responses, paths, session IDs, or credential material.
pub fn enable_claude_live_usage() -> Result<(), String> {
    let claude_directory = user_home_directory().join(".claude");
    let settings_path = claude_directory.join("settings.json");
    let bridge_directory = claude_directory.join("relay");
    install_claude_usage_bridge_in(&settings_path, &bridge_directory)
}

/// Open Claude Code's official interactive login flow in a visible terminal.
/// Relay never reads or submits the refresh credential itself.
pub fn reauthenticate_claude() -> Result<(), String> {
    spawn_claude_login_terminal()
}

fn install_claude_usage_bridge_in(
    settings_path: &Path,
    bridge_directory: &Path,
) -> Result<(), String> {
    let raw_settings = match fs::read_to_string(settings_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(_) => return Err("Could not read Claude Code settings".to_string()),
    };
    let mut settings = serde_json::from_str::<serde_json::Value>(&raw_settings)
        .map_err(|_| "Claude Code settings.json is not valid JSON".to_string())?;
    let root = settings
        .as_object_mut()
        .ok_or_else(|| "Claude Code settings.json must contain a JSON object".to_string())?;

    fs::create_dir_all(bridge_directory)
        .map_err(|_| "Could not create Relay's Claude integration directory".to_string())?;
    let bridge_path = bridge_directory.join(RELAY_STATUSLINE_BRIDGE_FILENAME);
    let bridge_command = format!("node \"{}\"", bridge_path.to_string_lossy());
    let config_path = bridge_directory.join(RELAY_STATUSLINE_CONFIG_FILENAME);

    let configured_command = root
        .get("statusLine")
        .and_then(serde_json::Value::as_object)
        .and_then(|statusline| statusline.get("command"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty());

    let upstream_command = if configured_command == Some(bridge_command.as_str()) {
        fs::read_to_string(&config_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<RelayStatuslineBridgeConfig>(&raw).ok())
            .map(|config| config.upstream_command)
            .filter(|command| !command.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_STATUSLINE_COMMAND.to_string())
    } else {
        configured_command
            .unwrap_or(DEFAULT_STATUSLINE_COMMAND)
            .to_string()
    };

    if upstream_command == bridge_command {
        return Err("Claude statusline bridge cannot forward to itself".to_string());
    }
    if upstream_command.len() > 4096 {
        return Err("Claude statusline command is too long to preserve safely".to_string());
    }

    let bridge_config = RelayStatuslineBridgeConfig {
        schema_version: 1,
        upstream_command,
    };
    let serialized_config = serde_json::to_vec_pretty(&bridge_config)
        .map_err(|_| "Could not serialize Relay's Claude integration".to_string())?;
    fs::write(&bridge_path, RELAY_STATUSLINE_BRIDGE_SOURCE)
        .map_err(|_| "Could not write Relay's Claude statusline bridge".to_string())?;
    fs::write(&config_path, serialized_config)
        .map_err(|_| "Could not preserve the existing Claude statusline command".to_string())?;

    let backup_path = bridge_directory.join(RELAY_STATUSLINE_BACKUP_FILENAME);
    if settings_path.is_file() && !backup_path.exists() {
        fs::write(&backup_path, raw_settings.as_bytes())
            .map_err(|_| "Could not back up Claude Code settings".to_string())?;
    }

    let statusline = root
        .entry("statusLine".to_string())
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Claude Code statusLine setting must be an object".to_string())?;
    statusline.insert("type".to_string(), serde_json::json!("command"));
    statusline.insert("command".to_string(), serde_json::json!(bridge_command));

    let serialized_settings = serde_json::to_vec_pretty(&settings)
        .map_err(|_| "Could not serialize Claude Code settings".to_string())?;
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Could not create the Claude Code settings directory".to_string())?;
    }
    fs::write(settings_path, serialized_settings)
        .map_err(|_| "Could not update Claude Code settings".to_string())?;
    Ok(())
}

/// Capture only Claude Code's documented live rate-limit fields for Relay.
///
/// A statusline bridge should call this with the original stdin bytes, then
/// forward those same bytes unchanged to `npx -y ccstatusline@latest`. The raw
/// payload is never written; only the allow-listed snapshot is persisted.
pub fn write_claude_usage_snapshot_from_statusline(
    statusline_json: &[u8],
) -> Result<ClaudeUsage, String> {
    let snapshot_path = user_home_directory()
        .join(".cache")
        .join("relay")
        .join("claude-usage.json");
    write_claude_usage_snapshot_from_statusline_to(
        statusline_json,
        &snapshot_path,
        SystemTime::now(),
    )
}

fn user_home_directory() -> PathBuf {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn claude_config_directory() -> PathBuf {
    env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| user_home_directory().join(".claude"))
}

fn read_claude_oauth_credentials_from(
    credentials_path: &Path,
    now: SystemTime,
) -> Result<ClaudeOauthCredentials, ClaudeUsageReason> {
    let metadata = fs::metadata(credentials_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ClaudeUsageReason::NoCredentials
        } else {
            ClaudeUsageReason::CacheUnreadable
        }
    })?;
    if !metadata.is_file() || metadata.len() > MAX_CREDENTIAL_BYTES {
        return Err(ClaudeUsageReason::CacheUnreadable);
    }
    let file = File::open(credentials_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ClaudeUsageReason::NoCredentials
        } else {
            ClaudeUsageReason::CacheUnreadable
        }
    })?;
    let reader = BufReader::new(file.take(MAX_CREDENTIAL_BYTES + 1));
    let credentials = serde_json::from_reader::<_, ClaudeCredentialsFile>(reader)
        .map_err(|_| ClaudeUsageReason::ParseError)?
        .claude_ai_oauth
        .filter(|credentials| !credentials.access_token.trim().is_empty())
        .ok_or(ClaudeUsageReason::NoCredentials)?;

    if credentials.expires_at <= unix_millis(now) {
        return Err(ClaudeUsageReason::AuthenticationExpired);
    }

    Ok(credentials)
}

fn parse_claude_usage_api_response(
    response: &[u8],
    captured_at: SystemTime,
) -> Result<ClaudeUsage, ClaudeUsageReason> {
    let parsed = serde_json::from_slice::<ClaudeUsageApiResponse>(response)
        .map_err(|_| ClaudeUsageReason::ParseError)?;
    let session_limit = parsed.limits.iter().find(|limit| limit.kind == "session");
    let weekly_limit = parsed
        .limits
        .iter()
        .find(|limit| limit.kind == "weekly_all");
    let session_percent = parsed
        .five_hour
        .as_ref()
        .and_then(|window| window.utilization)
        .or_else(|| session_limit.and_then(|limit| limit.utilization))
        .and_then(validate_usage_percent);
    let session_reset_at_ms = parsed
        .five_hour
        .as_ref()
        .and_then(|window| window.resets_at.as_deref())
        .or_else(|| session_limit.and_then(|limit| limit.resets_at.as_deref()))
        .and_then(parse_rfc3339_millis);
    let weekly_percent = parsed
        .seven_day
        .as_ref()
        .and_then(|window| window.utilization)
        .or_else(|| weekly_limit.and_then(|limit| limit.utilization))
        .and_then(validate_usage_percent);
    let weekly_reset_at_ms = parsed
        .seven_day
        .as_ref()
        .and_then(|window| window.resets_at.as_deref())
        .or_else(|| weekly_limit.and_then(|limit| limit.resets_at.as_deref()))
        .and_then(parse_rfc3339_millis);

    if session_percent.is_none()
        && session_reset_at_ms.is_none()
        && weekly_percent.is_none()
        && weekly_reset_at_ms.is_none()
    {
        return Err(ClaudeUsageReason::NoUsageData);
    }

    Ok(ClaudeUsage {
        source: ACCOUNT_USAGE_SOURCE,
        status: ClaudeUsageStatus::Available,
        reason: None,
        session_percent,
        session_reset_at_ms,
        weekly_percent,
        weekly_reset_at_ms,
        updated_at_ms: Some(unix_millis(captured_at)),
        age_seconds: Some(0),
    })
}

fn fetch_claude_usage_from_endpoint(
    credentials_path: &Path,
    snapshot_path: &Path,
    endpoint: &str,
    captured_at: SystemTime,
) -> ClaudeUsage {
    let credentials = match read_claude_oauth_credentials_from(credentials_path, captured_at) {
        Ok(credentials) => credentials,
        Err(reason) => {
            return empty_usage_with_source(
                ACCOUNT_USAGE_SOURCE,
                if reason == ClaudeUsageReason::NoCredentials {
                    ClaudeUsageStatus::Unavailable
                } else {
                    ClaudeUsageStatus::Error
                },
                reason,
            )
        }
    };

    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Relay/0.1")
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return empty_usage_with_source(
                ACCOUNT_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                ClaudeUsageReason::ApiError,
            )
        }
    };

    let response = match client
        .get(endpoint)
        .bearer_auth(&credentials.access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            return empty_usage_with_source(
                ACCOUNT_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                if error.is_timeout() {
                    ClaudeUsageReason::Timeout
                } else {
                    ClaudeUsageReason::ApiError
                },
            )
        }
    };

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return empty_usage_with_source(
            ACCOUNT_USAGE_SOURCE,
            ClaudeUsageStatus::Error,
            ClaudeUsageReason::AuthenticationExpired,
        );
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return empty_usage_with_source(
            ACCOUNT_USAGE_SOURCE,
            ClaudeUsageStatus::Error,
            ClaudeUsageReason::RateLimited,
        );
    }
    if !status.is_success() {
        return empty_usage_with_source(
            ACCOUNT_USAGE_SOURCE,
            ClaudeUsageStatus::Error,
            ClaudeUsageReason::ApiError,
        );
    }

    let mut response_body = Vec::new();
    if response
        .take(MAX_USAGE_CACHE_BYTES + 1)
        .read_to_end(&mut response_body)
        .is_err()
        || response_body.len() as u64 > MAX_USAGE_CACHE_BYTES
    {
        return empty_usage_with_source(
            ACCOUNT_USAGE_SOURCE,
            ClaudeUsageStatus::Error,
            ClaudeUsageReason::ParseError,
        );
    }

    let usage = match parse_claude_usage_api_response(&response_body, captured_at) {
        Ok(usage) => usage,
        Err(reason) => {
            return empty_usage_with_source(
                ACCOUNT_USAGE_SOURCE,
                if reason == ClaudeUsageReason::NoUsageData {
                    ClaudeUsageStatus::Unavailable
                } else {
                    ClaudeUsageStatus::Error
                },
                reason,
            )
        }
    };
    let snapshot = RelayClaudeUsageSnapshot {
        source: RelayUsageSnapshotSource::AccountApi,
        captured_at_ms: usage
            .updated_at_ms
            .unwrap_or_else(|| unix_millis(captured_at)),
        session_percent: usage.session_percent,
        session_reset_at_ms: usage.session_reset_at_ms,
        weekly_percent: usage.weekly_percent,
        weekly_reset_at_ms: usage.weekly_reset_at_ms,
    };
    if let Ok(serialized) = serde_json::to_vec(&snapshot) {
        if let Some(parent) = snapshot_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(snapshot_path, serialized);
    }
    usage
}

fn claude_projects_directory() -> PathBuf {
    claude_config_directory().join("projects")
}

#[cfg(test)]
fn read_claude_usage_with_direct_refresh(
    snapshot_path: &Path,
    ccstatusline_cache_path: &Path,
    credentials_path: &Path,
    endpoint: &str,
    now: SystemTime,
) -> ClaudeUsage {
    let local = read_claude_usage_from_sources(snapshot_path, ccstatusline_cache_path, now);
    if local.status == ClaudeUsageStatus::Available {
        return local;
    }
    fetch_claude_usage_from_endpoint(credentials_path, snapshot_path, endpoint, now)
}

fn read_claude_usage_from_sources(
    snapshot_path: &Path,
    ccstatusline_cache_path: &Path,
    now: SystemTime,
) -> ClaudeUsage {
    let live = read_live_usage_snapshot_from(snapshot_path, now);
    if live.status == ClaudeUsageStatus::Available {
        return live;
    }

    let fallback = read_claude_usage_from(ccstatusline_cache_path, now);
    match (usage_has_data(&live), usage_has_data(&fallback)) {
        (true, true) => {
            if fallback.updated_at_ms > live.updated_at_ms {
                fallback
            } else {
                live
            }
        }
        (true, false) => live,
        (false, true) => fallback,
        (false, false) => {
            if live.status == ClaudeUsageStatus::Error {
                live
            } else {
                fallback
            }
        }
    }
}

fn usage_has_data(usage: &ClaudeUsage) -> bool {
    usage.session_percent.is_some()
        || usage.session_reset_at_ms.is_some()
        || usage.weekly_percent.is_some()
        || usage.weekly_reset_at_ms.is_some()
}

fn read_live_usage_snapshot_from(snapshot_path: &Path, now: SystemTime) -> ClaudeUsage {
    let metadata = match fs::metadata(snapshot_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return empty_usage_with_source(
                LIVE_USAGE_SOURCE,
                ClaudeUsageStatus::Unavailable,
                ClaudeUsageReason::LiveSnapshotNotFound,
            )
        }
        Err(_) => {
            return empty_usage_with_source(
                LIVE_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                ClaudeUsageReason::CacheUnreadable,
            )
        }
    };
    if !metadata.is_file() || metadata.len() > MAX_USAGE_CACHE_BYTES {
        return empty_usage_with_source(
            LIVE_USAGE_SOURCE,
            ClaudeUsageStatus::Error,
            if metadata.len() > MAX_USAGE_CACHE_BYTES {
                ClaudeUsageReason::CacheTooLarge
            } else {
                ClaudeUsageReason::CacheUnreadable
            },
        );
    }
    let raw = match fs::read_to_string(snapshot_path) {
        Ok(raw) => raw,
        Err(_) => {
            return empty_usage_with_source(
                LIVE_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                ClaudeUsageReason::CacheUnreadable,
            )
        }
    };
    let parsed = match serde_json::from_str::<RelayClaudeUsageSnapshot>(&raw) {
        Ok(parsed) => parsed,
        Err(_) => {
            return empty_usage_with_source(
                LIVE_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                ClaudeUsageReason::InvalidCache,
            )
        }
    };
    usage_from_live_snapshot(parsed, now)
}

fn write_claude_usage_snapshot_from_statusline_to(
    statusline_json: &[u8],
    snapshot_path: &Path,
    captured_at: SystemTime,
) -> Result<ClaudeUsage, String> {
    if statusline_json.len() > MAX_STATUSLINE_INPUT_BYTES {
        return Err("Claude statusline payload is too large".to_string());
    }
    let input = serde_json::from_slice::<ClaudeStatuslineUsageInput>(statusline_json)
        .map_err(|_| "Claude statusline payload is invalid".to_string())?;
    let Some(rate_limits) = input.rate_limits else {
        return Ok(empty_usage_with_source(
            LIVE_USAGE_SOURCE,
            ClaudeUsageStatus::Unavailable,
            ClaudeUsageReason::NoUsageData,
        ));
    };

    let session_percent = rate_limits
        .five_hour
        .as_ref()
        .and_then(|window| window.used_percentage)
        .and_then(validate_usage_percent);
    let session_reset_at_ms = rate_limits
        .five_hour
        .as_ref()
        .and_then(|window| window.resets_at)
        .and_then(epoch_seconds_to_millis);
    let weekly_percent = rate_limits
        .seven_day
        .as_ref()
        .and_then(|window| window.used_percentage)
        .and_then(validate_usage_percent);
    let weekly_reset_at_ms = rate_limits
        .seven_day
        .as_ref()
        .and_then(|window| window.resets_at)
        .and_then(epoch_seconds_to_millis);

    if session_percent.is_none()
        && session_reset_at_ms.is_none()
        && weekly_percent.is_none()
        && weekly_reset_at_ms.is_none()
    {
        return Ok(empty_usage_with_source(
            LIVE_USAGE_SOURCE,
            ClaudeUsageStatus::Unavailable,
            ClaudeUsageReason::NoUsageData,
        ));
    }

    let snapshot = RelayClaudeUsageSnapshot {
        source: RelayUsageSnapshotSource::Statusline,
        captured_at_ms: unix_millis(captured_at),
        session_percent,
        session_reset_at_ms,
        weekly_percent,
        weekly_reset_at_ms,
    };
    let serialized = serde_json::to_vec(&snapshot)
        .map_err(|_| "Could not serialize Claude usage snapshot".to_string())?;
    let parent = snapshot_path
        .parent()
        .ok_or_else(|| "Claude usage snapshot path is invalid".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|_| "Could not create Claude usage snapshot directory".to_string())?;
    fs::write(snapshot_path, serialized)
        .map_err(|_| "Could not write Claude usage snapshot".to_string())?;

    Ok(usage_from_live_snapshot(snapshot, captured_at))
}

fn usage_from_live_snapshot(snapshot: RelayClaudeUsageSnapshot, now: SystemTime) -> ClaudeUsage {
    let source = snapshot.source.as_str();
    let captured_at = UNIX_EPOCH + Duration::from_millis(snapshot.captured_at_ms);
    let age = now.duration_since(captured_at).unwrap_or_default();
    let session_percent = snapshot.session_percent.and_then(validate_usage_percent);
    let weekly_percent = snapshot.weekly_percent.and_then(validate_usage_percent);
    let session_reset_at_ms = snapshot.session_reset_at_ms.filter(|value| *value > 0);
    let weekly_reset_at_ms = snapshot.weekly_reset_at_ms.filter(|value| *value > 0);

    if session_percent.is_none()
        && session_reset_at_ms.is_none()
        && weekly_percent.is_none()
        && weekly_reset_at_ms.is_none()
    {
        return empty_usage_with_source(
            source,
            ClaudeUsageStatus::Error,
            ClaudeUsageReason::InvalidCache,
        );
    }

    ClaudeUsage {
        source,
        status: if age <= CCSTATUSLINE_CACHE_MAX_AGE {
            ClaudeUsageStatus::Available
        } else {
            ClaudeUsageStatus::Stale
        },
        reason: None,
        session_percent,
        session_reset_at_ms,
        weekly_percent,
        weekly_reset_at_ms,
        updated_at_ms: Some(snapshot.captured_at_ms),
        age_seconds: Some(age.as_secs()),
    }
}

fn epoch_seconds_to_millis(seconds: f64) -> Option<u64> {
    let milliseconds = seconds * 1000.0;
    if !milliseconds.is_finite() || milliseconds < 0.0 || milliseconds > u64::MAX as f64 {
        None
    } else {
        Some(milliseconds.round() as u64)
    }
}

fn read_claude_usage_from(cache_path: &Path, now: SystemTime) -> ClaudeUsage {
    let metadata = match fs::metadata(cache_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return empty_usage_with_source(
                CCSTATUSLINE_USAGE_SOURCE,
                ClaudeUsageStatus::Unavailable,
                ClaudeUsageReason::CacheNotFound,
            )
        }
        Err(_) => {
            return empty_usage_with_source(
                CCSTATUSLINE_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                ClaudeUsageReason::CacheUnreadable,
            )
        }
    };
    if !metadata.is_file() {
        return empty_usage_with_source(
            CCSTATUSLINE_USAGE_SOURCE,
            ClaudeUsageStatus::Error,
            ClaudeUsageReason::CacheUnreadable,
        );
    }
    if metadata.len() > MAX_USAGE_CACHE_BYTES {
        return empty_usage_with_source(
            CCSTATUSLINE_USAGE_SOURCE,
            ClaudeUsageStatus::Error,
            ClaudeUsageReason::CacheTooLarge,
        );
    }

    let modified = match metadata.modified() {
        Ok(modified) => modified,
        Err(_) => {
            return empty_usage_with_source(
                CCSTATUSLINE_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                ClaudeUsageReason::CacheUnreadable,
            )
        }
    };
    let raw = match fs::read_to_string(cache_path) {
        Ok(raw) => raw,
        Err(_) => {
            return empty_usage_with_source(
                CCSTATUSLINE_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                ClaudeUsageReason::CacheUnreadable,
            )
        }
    };
    let parsed = match serde_json::from_str::<CcstatuslineUsageCache>(&raw) {
        Ok(parsed) => parsed,
        Err(_) => {
            return empty_usage_with_source(
                CCSTATUSLINE_USAGE_SOURCE,
                ClaudeUsageStatus::Error,
                ClaudeUsageReason::InvalidCache,
            )
        }
    };

    let session_percent = parsed.session_usage.and_then(validate_usage_percent);
    let weekly_percent = parsed.weekly_usage.and_then(validate_usage_percent);
    let session_reset_at_ms = parsed
        .session_reset_at
        .as_deref()
        .and_then(parse_rfc3339_millis);
    let weekly_reset_at_ms = parsed
        .weekly_reset_at
        .as_deref()
        .and_then(parse_rfc3339_millis);
    let upstream_error = parsed.error.as_deref().map(map_ccstatusline_error);
    let age = now.duration_since(modified).unwrap_or_default();
    let updated_at_ms = unix_millis(modified);

    if session_percent.is_none()
        && weekly_percent.is_none()
        && session_reset_at_ms.is_none()
        && weekly_reset_at_ms.is_none()
    {
        let reason = upstream_error.unwrap_or(ClaudeUsageReason::NoUsageData);
        return ClaudeUsage {
            source: CCSTATUSLINE_USAGE_SOURCE,
            status: if upstream_error.is_some() {
                ClaudeUsageStatus::Error
            } else {
                ClaudeUsageStatus::Unavailable
            },
            reason: Some(reason),
            session_percent: None,
            session_reset_at_ms: None,
            weekly_percent: None,
            weekly_reset_at_ms: None,
            updated_at_ms: Some(updated_at_ms),
            age_seconds: Some(age.as_secs()),
        };
    }

    ClaudeUsage {
        source: CCSTATUSLINE_USAGE_SOURCE,
        status: if upstream_error.is_some() || age > CCSTATUSLINE_CACHE_MAX_AGE {
            ClaudeUsageStatus::Stale
        } else {
            ClaudeUsageStatus::Available
        },
        reason: upstream_error,
        session_percent,
        session_reset_at_ms,
        weekly_percent,
        weekly_reset_at_ms,
        updated_at_ms: Some(updated_at_ms),
        age_seconds: Some(age.as_secs()),
    }
}

fn empty_usage_with_source(
    source: &'static str,
    status: ClaudeUsageStatus,
    reason: ClaudeUsageReason,
) -> ClaudeUsage {
    ClaudeUsage {
        source,
        status,
        reason: Some(reason),
        session_percent: None,
        session_reset_at_ms: None,
        weekly_percent: None,
        weekly_reset_at_ms: None,
        updated_at_ms: None,
        age_seconds: None,
    }
}

fn validate_usage_percent(percent: f64) -> Option<f64> {
    percent
        .is_finite()
        .then_some(percent)
        .filter(|value| (0.0..=100.0).contains(value))
}

fn map_ccstatusline_error(error: &str) -> ClaudeUsageReason {
    match error {
        "timeout" => ClaudeUsageReason::Timeout,
        "rate-limited" => ClaudeUsageReason::RateLimited,
        "api-error" => ClaudeUsageReason::ApiError,
        "parse-error" => ClaudeUsageReason::ParseError,
        "no-credentials" => ClaudeUsageReason::NoCredentials,
        _ => ClaudeUsageReason::UnknownUpstreamError,
    }
}

fn discover_claude_sessions_in(
    projects_directory: &Path,
    now: SystemTime,
    cli_available: bool,
) -> ClaudeDiscovery {
    let projects_directory_exists = projects_directory.is_dir();
    let mut detection = ClaudeDetection {
        source: SOURCE,
        installed: projects_directory_exists || cli_available,
        cli_available,
        projects_directory: projects_directory.to_path_buf(),
        projects_directory_exists,
        session_files_seen: 0,
        sessions_discovered: 0,
        unreadable_files: 0,
        skipped_nested_directories: 0,
    };
    let mut sessions = Vec::new();

    let project_entries = match fs::read_dir(projects_directory) {
        Ok(entries) => entries,
        Err(_) => {
            return ClaudeDiscovery {
                detection,
                sessions,
            }
        }
    };

    for project_entry in project_entries.flatten() {
        let Ok(project_type) = project_entry.file_type() else {
            continue;
        };
        if !project_type.is_dir() {
            continue;
        }

        let Ok(entries) = fs::read_dir(project_entry.path()) else {
            detection.unreadable_files += 1;
            continue;
        };

        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                detection.unreadable_files += 1;
                continue;
            };

            // Top-level sessions are `<project>/<session-id>.jsonl`. Claude's
            // nested `<session-id>/subagents/*.jsonl` files are intentionally
            // never traversed.
            if file_type.is_dir() {
                detection.skipped_nested_directories += 1;
                continue;
            }
            if !file_type.is_file() || entry.path().extension() != Some(OsStr::new("jsonl")) {
                continue;
            }

            detection.session_files_seen += 1;
            match parse_session_file(&entry.path()) {
                Ok(Some(parsed)) => sessions.push(summarize_session(parsed, now, cli_available)),
                Ok(None) => {}
                Err(_) => detection.unreadable_files += 1,
            }
        }
    }

    sessions.sort_by(|left, right| {
        right
            .last_activity_ms
            .cmp(&left.last_activity_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    detection.sessions_discovered = sessions.len();

    ClaudeDiscovery {
        detection,
        sessions,
    }
}

fn parse_session_file(path: &Path) -> Result<Option<ParsedSession>, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    let modified = metadata.modified().map_err(|error| error.to_string())?;
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(file);
    let mut record = Vec::new();
    let mut id = None;
    let mut cwd: Option<(PathBuf, Option<u64>, usize)> = None;
    let mut branch: Option<(String, Option<u64>, usize)> = None;
    let mut ai_title = None;
    let mut agent_name = None;
    let mut slug = None;
    let mut safe_activity = None;
    let mut max_timestamp_ms = None;
    let mut sequence = 0;

    loop {
        record.clear();
        let bytes_read = reader
            .read_until(b'\n', &mut record)
            .map_err(|error| error.to_string())?;
        if bytes_read == 0 {
            break;
        }
        if record.len() > MAX_RECORD_BYTES {
            continue;
        }

        let Ok(metadata) = serde_json::from_slice::<ClaudeRecordMetadata>(&record) else {
            // A partially written final line should not hide an otherwise valid
            // session. Claude can append while Relay scans.
            continue;
        };
        if metadata.is_sidechain {
            continue;
        }
        if let Some(record_id) = metadata.session_id.as_deref() {
            if validate_session_id(record_id).is_err() {
                continue;
            }
            match id.as_deref() {
                None => id = Some(record_id.to_owned()),
                Some(authoritative_id) if authoritative_id != record_id => continue,
                Some(_) => {}
            }
        }
        let timestamp_ms = metadata.timestamp.as_deref().and_then(parse_rfc3339_millis);
        if let Some(timestamp_ms) = timestamp_ms {
            max_timestamp_ms = Some(
                max_timestamp_ms.map_or(timestamp_ms, |current: u64| current.max(timestamp_ms)),
            );
        }
        if let Some(candidate) = metadata.cwd.filter(|candidate| candidate.is_absolute()) {
            if candidate_is_newer(
                cwd.as_ref().map(|(_, time, order)| (*time, *order)),
                timestamp_ms,
                sequence,
            ) {
                cwd = Some((candidate, timestamp_ms, sequence));
            }
        }
        if let Some(candidate) = metadata.git_branch.and_then(sanitize_branch) {
            if candidate_is_newer(
                branch.as_ref().map(|(_, time, order)| (*time, *order)),
                timestamp_ms,
                sequence,
            ) {
                branch = Some((candidate, timestamp_ms, sequence));
            }
        }
        if let Some(candidate) = metadata
            .ai_title
            .and_then(|value| sanitize_label(value, 160))
        {
            ai_title = Some(candidate);
        }
        if let Some(candidate) = metadata
            .agent_name
            .and_then(|value| sanitize_label(value, 80))
        {
            agent_name = Some(candidate);
        }
        if let Some(candidate) = metadata.slug.and_then(|value| sanitize_label(value, 160)) {
            slug = Some(candidate);
        }
        if matches!(metadata.record_type.as_deref(), Some("assistant" | "user")) {
            safe_activity = metadata.message.as_ref().and_then(safe_tool_activity);
        }
        sequence += 1;
    }

    let Some(id) = id else {
        return Ok(None);
    };
    let Some((cwd, _, _)) = cwd else {
        return Ok(None);
    };
    let last_activity = max_timestamp_ms
        .map(|millis| UNIX_EPOCH + Duration::from_millis(millis))
        .unwrap_or(modified);

    Ok(Some(ParsedSession {
        id,
        cwd,
        branch: branch.map(|(branch, _, _)| branch),
        session_name: ai_title.or(slug),
        agent_name,
        safe_activity,
        last_activity,
    }))
}

fn candidate_is_newer(
    current: Option<(Option<u64>, usize)>,
    candidate_timestamp: Option<u64>,
    candidate_sequence: usize,
) -> bool {
    let Some((current_timestamp, current_sequence)) = current else {
        return true;
    };
    match (candidate_timestamp, current_timestamp) {
        (Some(candidate), Some(current)) => {
            candidate > current || (candidate == current && candidate_sequence > current_sequence)
        }
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => candidate_sequence > current_sequence,
    }
}

fn parse_rfc3339_millis(timestamp: &str) -> Option<u64> {
    let parsed = OffsetDateTime::parse(timestamp, &Rfc3339).ok()?;
    let nanos = parsed.unix_timestamp_nanos();
    if nanos < 0 {
        return None;
    }
    u64::try_from(nanos / 1_000_000).ok()
}

fn summarize_session(
    parsed: ParsedSession,
    now: SystemTime,
    cli_available: bool,
) -> ClaudeSessionSummary {
    let repository = find_repository_root(&parsed.cwd);
    let project = repository
        .as_deref()
        .unwrap_or(&parsed.cwd)
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Local project")
        .to_owned();
    let age = now.duration_since(parsed.last_activity).unwrap_or_default();
    let state = if age <= RECENT_ACTIVITY_WINDOW {
        ClaudeSessionState::Recent
    } else {
        ClaudeSessionState::Idle
    };

    ClaudeSessionSummary {
        id: parsed.id,
        source: SOURCE,
        title: format!("{project} — Claude Code"),
        session_name: parsed.session_name,
        agent_name: parsed.agent_name,
        safe_activity: parsed.safe_activity,
        project,
        resume_available: cli_available && parsed.cwd.is_dir(),
        cwd: parsed.cwd,
        repository,
        branch: parsed.branch,
        last_activity_ms: unix_millis(parsed.last_activity),
        state,
    }
}

fn find_repository_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|directory| directory.join(".git").exists())
        .map(Path::to_path_buf)
}

fn sanitize_branch(branch: String) -> Option<String> {
    let trimmed = branch.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed.chars().any(char::is_control) {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn sanitize_label(value: String, max_len: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > max_len || trimmed.chars().any(char::is_control) {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn safe_tool_activity(message: &ClaudeMessageMetadata) -> Option<String> {
    let name = message
        .content
        .iter()
        .rev()
        .find(|block| block.block_type.as_deref() == Some("tool_use"))?
        .name
        .as_deref()?;
    let label = match name {
        "Read" => "Reading files",
        "Glob" | "Grep" => "Searching files",
        "Edit" | "Write" => "Editing files",
        "Bash" => "Running a command",
        "WebSearch" => "Searching web",
        "WebFetch" => "Reading page",
        "Agent" => "Delegating work",
        "AskUserQuestion" => "Waiting for input",
        "Skill" => "Running a workflow",
        _ => "Using a tool",
    };
    Some(label.to_string())
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.len() != 36 {
        return Err("Claude session id must be a canonical UUID".to_string());
    }
    for (index, byte) in session_id.bytes().enumerate() {
        let is_separator = matches!(index, 8 | 13 | 18 | 23);
        if (is_separator && byte != b'-') || (!is_separator && !byte.is_ascii_hexdigit()) {
            return Err("Claude session id must be a canonical UUID".to_string());
        }
    }
    Ok(())
}

fn validate_working_directory(cwd: &str) -> Result<PathBuf, String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return Err("Claude working directory must be between 1 and 2048 characters".to_string());
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err("Claude working directory must be an absolute path".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "Claude working directory does not exist".to_string())?;
    if !canonical.is_dir() {
        return Err("Claude working directory must point to a directory".to_string());
    }
    Ok(canonical)
}

fn claude_cli_available() -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };

    #[cfg(windows)]
    const CANDIDATES: &[&str] = &["claude.exe", "claude.cmd", "claude.bat"];
    #[cfg(not(windows))]
    const CANDIDATES: &[&str] = &["claude"];

    env::split_paths(&path).any(|directory| {
        CANDIDATES
            .iter()
            .any(|candidate| directory.join(candidate).is_file())
    })
}

#[cfg(windows)]
fn spawn_resume_terminal(session_id: &str, cwd: &Path) -> Result<(), String> {
    let args = terminal_resume_args(session_id, cwd);
    match Command::new("wt.exe").args(&args).current_dir(cwd).spawn() {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            use std::os::windows::process::CommandExt;

            // Fall back to a visible PowerShell console when Windows Terminal is
            // unavailable. The script is constant; the validated UUID remains a
            // separate positional argument.
            Command::new("powershell.exe")
                .args(powershell_resume_args(session_id))
                .current_dir(cwd)
                .creation_flags(0x0000_0010) // CREATE_NEW_CONSOLE
                .spawn()
                .map(|_| ())
                .map_err(|fallback| {
                    format!("Could not open Windows Terminal ({error}) or PowerShell ({fallback})")
                })
        }
        Err(error) => Err(format!("Could not open Windows Terminal: {error}")),
    }
}

#[cfg(not(windows))]
fn spawn_resume_terminal(_session_id: &str, _cwd: &Path) -> Result<(), String> {
    Err("Opening Claude Code sessions is currently supported on Windows only".to_string())
}

#[cfg(windows)]
fn spawn_claude_login_terminal() -> Result<(), String> {
    match Command::new("wt.exe").args(claude_login_args()).spawn() {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            use std::os::windows::process::CommandExt;

            Command::new("powershell.exe")
                .args(powershell_login_args())
                .creation_flags(0x0000_0010) // CREATE_NEW_CONSOLE
                .spawn()
                .map(|_| ())
                .map_err(|fallback| {
                    format!("Could not open Windows Terminal ({error}) or PowerShell ({fallback})")
                })
        }
        Err(error) => Err(format!("Could not open Windows Terminal: {error}")),
    }
}

#[cfg(not(windows))]
fn spawn_claude_login_terminal() -> Result<(), String> {
    Err("Opening Claude Code login is currently supported on Windows only".to_string())
}

#[cfg(windows)]
fn claude_login_args() -> Vec<OsString> {
    let mut args = vec![OsString::from("new-tab"), OsString::from("powershell.exe")];
    args.extend(powershell_login_args());
    args
}

#[cfg(windows)]
fn powershell_login_args() -> Vec<OsString> {
    vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoExit"),
        OsString::from("-Command"),
        OsString::from("& claude auth login"),
    ]
}

#[cfg(windows)]
fn terminal_resume_args(session_id: &str, cwd: &Path) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("new-tab"),
        OsString::from("--startingDirectory"),
        cwd.as_os_str().to_owned(),
        OsString::from("powershell.exe"),
    ];
    args.extend(powershell_resume_args(session_id));
    args
}

#[cfg(windows)]
fn powershell_resume_args(session_id: &str) -> Vec<OsString> {
    vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoExit"),
        OsString::from("-Command"),
        OsString::from("& claude --resume $args[0]"),
        OsString::from(session_id),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        claude_login_args, discover_claude_sessions_in, fetch_claude_usage_from_endpoint,
        install_claude_usage_bridge_in, parse_claude_usage_api_response,
        read_claude_oauth_credentials_from, read_claude_usage_from, read_claude_usage_from_sources,
        read_claude_usage_with_direct_refresh, read_live_usage_snapshot_from, validate_session_id,
        validate_working_directory, write_claude_usage_snapshot_from_statusline_to,
        ClaudeSessionState, ClaudeUsageReason, ClaudeUsageStatus, RelayStatuslineBridgeConfig,
        RELAY_STATUSLINE_BACKUP_FILENAME, RELAY_STATUSLINE_BRIDGE_FILENAME,
        RELAY_STATUSLINE_CONFIG_FILENAME, SOURCE,
    };
    use std::{
        ffi::OsString,
        fs,
        io::{Read, Write},
        net::TcpListener,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::{Duration, UNIX_EPOCH},
    };

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);
    const SESSION_ID: &str = "12345678-1234-4abc-8def-1234567890ab";

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let unique = format!(
                "relay-claude-test-{}-{}",
                std::process::id(),
                FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let root = std::env::temp_dir().join(unique);
            fs::create_dir_all(&root).expect("fixture directory");
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn one_request_server(status: &str, body: &str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("local test server");
        let address = listener.local_addr().expect("server address");
        let status = status.to_string();
        let body = body.to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("usage request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            )
            .expect("write response");
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{address}/api/oauth/usage"), handle)
    }

    #[test]
    fn reads_only_allowlisted_ccstatusline_usage_fields() {
        let fixture = Fixture::new();
        let cache = fixture.path().join("usage.json");
        fs::write(
            &cache,
            concat!(
                "{",
                "\"sessionUsage\":0.0,",
                "\"sessionResetAt\":\"2026-09-20T23:49:00Z\",",
                "\"weeklyUsage\":15.0,",
                "\"weeklyResetAt\":\"2026-09-25T12:30:00Z\",",
                "\"tokenHash\":\"DO-NOT-RETURN-THIS-FINGERPRINT\",",
                "\"unknownSensitiveField\":\"DO-NOT-RETURN-THIS-EITHER\"",
                "}"
            ),
        )
        .expect("usage cache fixture");
        let modified = fs::metadata(&cache)
            .expect("cache metadata")
            .modified()
            .expect("cache modified time");

        let usage = read_claude_usage_from(&cache, modified + Duration::from_secs(10));
        assert_eq!(usage.status, ClaudeUsageStatus::Available);
        assert_eq!(usage.reason, None);
        assert_eq!(usage.session_percent, Some(0.0));
        assert_eq!(usage.weekly_percent, Some(15.0));
        assert_eq!(usage.session_reset_at_ms, Some(1_789_948_140_000));
        assert_eq!(usage.age_seconds, Some(10));

        let serialized = serde_json::to_string(&usage).expect("serializable usage");
        assert!(!serialized.contains("DO-NOT-RETURN"));
        assert!(!serialized.contains("tokenHash"));
        assert!(!serialized.contains("unknownSensitiveField"));
    }

    #[test]
    fn parses_only_allowlisted_direct_account_usage_fields() {
        let observed_at = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let usage = parse_claude_usage_api_response(
            br#"{
              "five_hour":{"utilization":12.5,"resets_at":"2027-01-15T09:00:00Z"},
              "seven_day":{"utilization":44.0,"resets_at":"2027-01-19T12:30:00Z"},
              "account_email":"private@example.com",
              "organization":{"name":"Private organization"}
            }"#,
            observed_at,
        )
        .expect("valid direct usage response");

        assert_eq!(usage.source, "claude-account-api");
        assert_eq!(usage.status, ClaudeUsageStatus::Available);
        assert_eq!(usage.session_percent, Some(12.5));
        assert_eq!(usage.weekly_percent, Some(44.0));
        assert_eq!(usage.session_reset_at_ms, Some(1_800_003_600_000));
        assert_eq!(usage.weekly_reset_at_ms, Some(1_800_361_800_000));

        let serialized = serde_json::to_string(&usage).expect("serializable usage");
        assert!(!serialized.contains("private@example.com"));
        assert!(!serialized.contains("Private organization"));
    }

    #[test]
    fn refuses_an_expired_oauth_access_token_before_network_use() {
        let fixture = Fixture::new();
        let credentials_path = fixture.path().join(".credentials.json");
        fs::write(
            &credentials_path,
            r#"{"claudeAiOauth":{"accessToken":"never-return-this","expiresAt":999}}"#,
        )
        .expect("credential fixture");

        let result = read_claude_oauth_credentials_from(
            &credentials_path,
            UNIX_EPOCH + Duration::from_secs(2),
        );
        assert!(matches!(
            result,
            Err(ClaudeUsageReason::AuthenticationExpired)
        ));
    }

    #[test]
    fn fetches_direct_usage_with_bearer_auth_and_persists_only_display_fields() {
        let fixture = Fixture::new();
        let credentials_path = fixture.path().join(".credentials.json");
        let snapshot_path = fixture.path().join("usage.json");
        fs::write(
            &credentials_path,
            r#"{"claudeAiOauth":{"accessToken":"test-secret-token","expiresAt":5000}}"#,
        )
        .expect("credential fixture");
        let (endpoint, server) = one_request_server(
            "200 OK",
            r#"{"five_hour":{"utilization":7.0},"seven_day":{"utilization":21.0}}"#,
        );
        let observed_at = UNIX_EPOCH + Duration::from_secs(2);

        let usage = fetch_claude_usage_from_endpoint(
            &credentials_path,
            &snapshot_path,
            &endpoint,
            observed_at,
        );
        let request = server.join().expect("server result");

        assert_eq!(usage.status, ClaudeUsageStatus::Available);
        assert_eq!(usage.session_percent, Some(7.0));
        assert_eq!(usage.weekly_percent, Some(21.0));
        assert!(
            request.contains("authorization: Bearer test-secret-token")
                || request.contains("Authorization: Bearer test-secret-token")
        );
        assert!(request.contains("anthropic-beta: oauth-2025-04-20"));

        let cached =
            read_live_usage_snapshot_from(&snapshot_path, observed_at + Duration::from_secs(10));
        assert_eq!(cached.source, "claude-account-api");
        assert_eq!(cached.status, ClaudeUsageStatus::Available);
        assert_eq!(cached.session_percent, Some(7.0));
        let stored = fs::read_to_string(snapshot_path).expect("stored snapshot");
        assert!(!stored.contains("test-secret-token"));
    }

    #[test]
    fn maps_direct_usage_unauthorized_to_expired_authentication() {
        let fixture = Fixture::new();
        let credentials_path = fixture.path().join(".credentials.json");
        fs::write(
            &credentials_path,
            r#"{"claudeAiOauth":{"accessToken":"test-secret-token","expiresAt":5000}}"#,
        )
        .expect("credential fixture");
        let (endpoint, server) = one_request_server(
            "401 Unauthorized",
            r#"{"type":"error","error":{"message":"expired"}}"#,
        );

        let usage = fetch_claude_usage_from_endpoint(
            &credentials_path,
            &fixture.path().join("usage.json"),
            &endpoint,
            UNIX_EPOCH + Duration::from_secs(2),
        );
        server.join().expect("server result");

        assert_eq!(usage.status, ClaudeUsageStatus::Error);
        assert_eq!(usage.reason, Some(ClaudeUsageReason::AuthenticationExpired));
        assert_eq!(usage.session_percent, None);
    }

    #[test]
    fn refreshes_directly_when_local_usage_sources_are_not_current() {
        let fixture = Fixture::new();
        let snapshot_path = fixture.path().join("relay-usage.json");
        let fallback_path = fixture.path().join("missing-ccstatusline.json");
        let credentials_path = fixture.path().join(".credentials.json");
        fs::write(
            &credentials_path,
            r#"{"claudeAiOauth":{"accessToken":"test-secret-token","expiresAt":5000}}"#,
        )
        .expect("credential fixture");
        let (endpoint, server) = one_request_server(
            "200 OK",
            r#"{"limits":[{"kind":"session","utilization":9.0},{"kind":"weekly_all","utilization":23.0}]}"#,
        );

        let usage = read_claude_usage_with_direct_refresh(
            &snapshot_path,
            &fallback_path,
            &credentials_path,
            &endpoint,
            UNIX_EPOCH + Duration::from_secs(2),
        );
        server.join().expect("server result");

        assert_eq!(usage.source, "claude-account-api");
        assert_eq!(usage.status, ClaudeUsageStatus::Available);
        assert_eq!(usage.session_percent, Some(9.0));
        assert_eq!(usage.weekly_percent, Some(23.0));
    }

    #[test]
    fn current_statusline_usage_avoids_direct_credential_access() {
        let fixture = Fixture::new();
        let snapshot_path = fixture.path().join("relay-usage.json");
        let observed_at = UNIX_EPOCH + Duration::from_secs(2);
        write_claude_usage_snapshot_from_statusline_to(
            br#"{"rate_limits":{"five_hour":{"used_percentage":3.0}}}"#,
            &snapshot_path,
            observed_at,
        )
        .expect("statusline snapshot");

        let usage = read_claude_usage_with_direct_refresh(
            &snapshot_path,
            &fixture.path().join("missing-ccstatusline.json"),
            &fixture.path().join("missing-credentials.json"),
            "http://127.0.0.1:1/should-not-be-called",
            observed_at + Duration::from_secs(10),
        );

        assert_eq!(usage.source, "claude-statusline");
        assert_eq!(usage.status, ClaudeUsageStatus::Available);
        assert_eq!(usage.session_percent, Some(3.0));
    }

    #[test]
    fn labels_old_usage_cache_as_stale() {
        let fixture = Fixture::new();
        let cache = fixture.path().join("usage.json");
        fs::write(&cache, "{\"sessionUsage\":5,\"weeklyUsage\":25}").expect("usage cache fixture");
        let modified = fs::metadata(&cache)
            .expect("cache metadata")
            .modified()
            .expect("cache modified time");

        let usage = read_claude_usage_from(&cache, modified + Duration::from_secs(181));
        assert_eq!(usage.status, ClaudeUsageStatus::Stale);
        assert_eq!(usage.session_percent, Some(5.0));
        assert_eq!(usage.weekly_percent, Some(25.0));
        assert_eq!(usage.age_seconds, Some(181));
    }

    #[test]
    fn reports_missing_invalid_and_upstream_error_usage_states() {
        let fixture = Fixture::new();
        let missing = read_claude_usage_from(&fixture.path().join("missing.json"), UNIX_EPOCH);
        assert_eq!(missing.status, ClaudeUsageStatus::Unavailable);
        assert_eq!(missing.reason, Some(ClaudeUsageReason::CacheNotFound));

        let invalid_path = fixture.path().join("invalid.json");
        fs::write(&invalid_path, "not-json").expect("invalid cache fixture");
        let invalid = read_claude_usage_from(&invalid_path, UNIX_EPOCH);
        assert_eq!(invalid.status, ClaudeUsageStatus::Error);
        assert_eq!(invalid.reason, Some(ClaudeUsageReason::InvalidCache));

        let error_path = fixture.path().join("error.json");
        fs::write(
            &error_path,
            "{\"error\":\"rate-limited\",\"tokenHash\":\"private\"}",
        )
        .expect("error cache fixture");
        let error = read_claude_usage_from(&error_path, UNIX_EPOCH);
        assert_eq!(error.status, ClaudeUsageStatus::Error);
        assert_eq!(error.reason, Some(ClaudeUsageReason::RateLimited));

        let stale_data_path = fixture.path().join("stale-data.json");
        fs::write(
            &stale_data_path,
            "{\"sessionUsage\":12,\"weeklyUsage\":34,\"error\":\"timeout\"}",
        )
        .expect("stale data cache fixture");
        let modified = fs::metadata(&stale_data_path)
            .expect("stale cache metadata")
            .modified()
            .expect("stale cache modified time");
        let stale_data = read_claude_usage_from(&stale_data_path, modified);
        assert_eq!(stale_data.status, ClaudeUsageStatus::Stale);
        assert_eq!(stale_data.reason, Some(ClaudeUsageReason::Timeout));
        assert_eq!(stale_data.session_percent, Some(12.0));
    }

    #[test]
    fn rejects_live_snapshot_without_any_valid_usage_fields() {
        let fixture = Fixture::new();
        let snapshot = fixture.path().join("relay-usage.json");
        let missing_fallback = fixture.path().join("missing-fallback.json");
        fs::write(
            &snapshot,
            concat!(
                "{",
                "\"capturedAtMs\":1000,",
                "\"sessionPercent\":101,",
                "\"sessionResetAtMs\":0,",
                "\"weeklyPercent\":-1,",
                "\"weeklyResetAtMs\":null",
                "}"
            ),
        )
        .expect("invalid live snapshot fixture");

        let usage = read_claude_usage_from_sources(
            &snapshot,
            &missing_fallback,
            UNIX_EPOCH + Duration::from_secs(2),
        );
        assert_eq!(usage.source, "claude-statusline");
        assert_eq!(usage.status, ClaudeUsageStatus::Error);
        assert_eq!(usage.reason, Some(ClaudeUsageReason::InvalidCache));
    }

    #[test]
    fn captures_only_allowlisted_live_rate_limits_and_prefers_them() {
        let fixture = Fixture::new();
        let snapshot = fixture.path().join("relay-usage.json");
        let fallback = fixture.path().join("ccstatusline-usage.json");
        fs::write(&fallback, "{\"sessionUsage\":90,\"weeklyUsage\":80}").expect("fallback fixture");
        let observed_at = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let payload = format!(
            concat!(
                "{{",
                "\"session_id\":\"{id}\",",
                "\"session_name\":\"Safe display name\",",
                "\"cwd\":\"C:/private/project\",",
                "\"transcript_path\":\"C:/private/transcript.jsonl\",",
                "\"prompt_id\":\"private-prompt-id\",",
                "\"rate_limits\":{{",
                "\"five_hour\":{{\"used_percentage\":0.0,\"resets_at\":1800003600}},",
                "\"seven_day\":{{\"used_percentage\":15.0,\"resets_at\":1800604800}}",
                "}}",
                "}}"
            ),
            id = SESSION_ID,
        );

        let written = write_claude_usage_snapshot_from_statusline_to(
            payload.as_bytes(),
            &snapshot,
            observed_at,
        )
        .expect("live usage snapshot");
        assert_eq!(written.status, ClaudeUsageStatus::Available);
        assert_eq!(written.session_percent, Some(0.0));
        assert_eq!(written.weekly_percent, Some(15.0));
        assert_eq!(written.session_reset_at_ms, Some(1_800_003_600_000));

        let stored = fs::read_to_string(&snapshot).expect("stored snapshot");
        assert!(!stored.contains("session_id"));
        assert!(!stored.contains("session_name"));
        assert!(!stored.contains("private"));
        assert!(!stored.contains("cwd"));
        assert!(!stored.contains("transcript"));
        assert!(!stored.contains("prompt"));

        let selected = read_claude_usage_from_sources(
            &snapshot,
            &fallback,
            observed_at + Duration::from_secs(20),
        );
        assert_eq!(selected.source, "claude-statusline");
        assert_eq!(selected.status, ClaudeUsageStatus::Available);
        assert_eq!(selected.weekly_percent, Some(15.0));
        assert_eq!(selected.age_seconds, Some(20));
    }

    #[test]
    fn statusline_payload_without_rate_limits_does_not_replace_snapshot() {
        let fixture = Fixture::new();
        let snapshot = fixture.path().join("relay-usage.json");
        fs::write(&snapshot, "existing snapshot").expect("existing snapshot fixture");

        let usage = write_claude_usage_snapshot_from_statusline_to(
            br#"{"session_id":"12345678-1234-4abc-8def-1234567890ab"}"#,
            &snapshot,
            UNIX_EPOCH,
        )
        .expect("payload without usage is valid");
        assert_eq!(usage.status, ClaudeUsageStatus::Unavailable);
        assert_eq!(usage.reason, Some(ClaudeUsageReason::NoUsageData));
        assert_eq!(
            fs::read_to_string(&snapshot).expect("unchanged snapshot"),
            "existing snapshot"
        );
    }

    #[test]
    fn installs_live_usage_bridge_without_losing_existing_settings() {
        let fixture = Fixture::new();
        let claude_directory = fixture.path().join(".claude");
        let settings_path = claude_directory.join("settings.json");
        let bridge_directory = claude_directory.join("relay");
        fs::create_dir_all(&claude_directory).expect("Claude fixture directory");
        let original = concat!(
            "{\n",
            "  \"theme\": \"dark\",\n",
            "  \"statusLine\": {\n",
            "    \"type\": \"command\",\n",
            "    \"command\": \"npx -y ccstatusline@latest\",\n",
            "    \"padding\": 0\n",
            "  }\n",
            "}\n"
        );
        fs::write(&settings_path, original).expect("settings fixture");

        install_claude_usage_bridge_in(&settings_path, &bridge_directory)
            .expect("bridge installation");

        let updated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).expect("updated settings"))
                .expect("valid updated settings");
        assert_eq!(updated["theme"], "dark");
        assert_eq!(updated["statusLine"]["padding"], 0);
        assert_eq!(updated["statusLine"]["type"], "command");
        assert!(updated["statusLine"]["command"]
            .as_str()
            .expect("bridge command")
            .contains(RELAY_STATUSLINE_BRIDGE_FILENAME));
        assert_eq!(
            fs::read_to_string(bridge_directory.join(RELAY_STATUSLINE_BACKUP_FILENAME))
                .expect("settings backup"),
            original
        );

        let config: RelayStatuslineBridgeConfig = serde_json::from_str(
            &fs::read_to_string(bridge_directory.join(RELAY_STATUSLINE_CONFIG_FILENAME))
                .expect("bridge config"),
        )
        .expect("valid bridge config");
        assert_eq!(config.schema_version, 1);
        assert_eq!(config.upstream_command, "npx -y ccstatusline@latest");

        let bridge = fs::read_to_string(bridge_directory.join(RELAY_STATUSLINE_BRIDGE_FILENAME))
            .expect("bridge script");
        assert!(bridge.contains("payload?.rate_limits?.five_hour"));
        assert!(bridge.contains("payload?.rate_limits?.seven_day"));
        assert!(!bridge.contains("session_id"));
        assert!(!bridge.contains("transcript_path"));
    }

    #[test]
    fn reinstalling_live_usage_bridge_is_idempotent() {
        let fixture = Fixture::new();
        let claude_directory = fixture.path().join(".claude");
        let settings_path = claude_directory.join("settings.json");
        let bridge_directory = claude_directory.join("relay");
        fs::create_dir_all(&claude_directory).expect("Claude fixture directory");
        fs::write(
            &settings_path,
            r#"{"statusLine":{"type":"command","command":"custom-statusline","padding":2}}"#,
        )
        .expect("settings fixture");

        install_claude_usage_bridge_in(&settings_path, &bridge_directory)
            .expect("first bridge installation");
        install_claude_usage_bridge_in(&settings_path, &bridge_directory)
            .expect("second bridge installation");

        let config: RelayStatuslineBridgeConfig = serde_json::from_str(
            &fs::read_to_string(bridge_directory.join(RELAY_STATUSLINE_CONFIG_FILENAME))
                .expect("bridge config"),
        )
        .expect("valid bridge config");
        assert_eq!(config.upstream_command, "custom-statusline");
    }

    #[test]
    fn discovers_only_top_level_sessions_and_never_returns_conversation_content() {
        let fixture = Fixture::new();
        let projects_root = fixture.path().join("projects");
        let project_dir = projects_root.join("C--work-relay");
        let cwd = fixture.path().join("work").join("relay");
        fs::create_dir_all(cwd.join(".git")).expect("repository fixture");
        fs::create_dir_all(project_dir.join(SESSION_ID).join("subagents"))
            .expect("subagent fixture");

        let transcript = format!(
            concat!(
                "{{\"type\":\"user\",\"sessionId\":\"{id}\",",
                "\"cwd\":{cwd:?},\"gitBranch\":\"main\",",
                "\"timestamp\":\"1970-01-01T00:00:10Z\",",
                "\"message\":{{\"content\":\"TOP SECRET PROMPT\"}}}}\n",
                "{{\"type\":\"assistant\",\"sessionId\":\"{id}\",",
                "\"timestamp\":\"1970-01-01T00:00:11Z\",",
                "\"message\":{{\"content\":\"TOP SECRET ANSWER\"}}}}\n",
                "{{\"type\":\"ai-title\",\"sessionId\":\"{id}\",",
                "\"aiTitle\":\"Fix Relay connector\"}}\n",
                "{{\"type\":\"agent-name\",\"sessionId\":\"{id}\",",
                "\"agentName\":\"backend-agent\"}}\n",
                "{{\"type\":\"assistant\",\"sessionId\":\"{id}\",",
                "\"message\":{{\"content\":[{{\"type\":\"tool_use\",",
                "\"id\":\"tool-safe-id\",\"name\":\"Grep\",",
                "\"input\":{{\"pattern\":\"NEVER RETURN TOOL INPUT\"}}}}]}}}}\n"
            ),
            id = SESSION_ID,
            cwd = cwd.to_string_lossy()
        );
        // Real Claude installations can contain filenames that differ from the
        // authoritative sessionId stored in records.
        fs::write(project_dir.join("mismatched-filename.jsonl"), transcript)
            .expect("top-level transcript");
        fs::write(
            project_dir
                .join(SESSION_ID)
                .join("subagents")
                .join("agent-deadbeef.jsonl"),
            "{}\n",
        )
        .expect("nested transcript");

        let discovery =
            discover_claude_sessions_in(&projects_root, UNIX_EPOCH + Duration::from_secs(12), true);
        assert_eq!(discovery.detection.session_files_seen, 1);
        assert_eq!(discovery.detection.skipped_nested_directories, 1);
        assert_eq!(discovery.sessions.len(), 1);
        let session = &discovery.sessions[0];
        assert_eq!(session.id, SESSION_ID);
        assert_eq!(session.source, SOURCE);
        assert_eq!(session.project, "relay");
        assert_eq!(session.title, "relay — Claude Code");
        assert_eq!(session.session_name.as_deref(), Some("Fix Relay connector"));
        assert_eq!(session.agent_name.as_deref(), Some("backend-agent"));
        assert_eq!(session.safe_activity.as_deref(), Some("Searching files"));
        assert_eq!(session.branch.as_deref(), Some("main"));
        assert_eq!(session.state, ClaudeSessionState::Recent);
        assert!(session.resume_available);

        let serialized = serde_json::to_string(&discovery).expect("serializable discovery");
        assert!(!serialized.contains("TOP SECRET PROMPT"));
        assert!(!serialized.contains("TOP SECRET ANSWER"));
        assert!(!serialized.contains("NEVER RETURN TOOL INPUT"));
    }

    #[test]
    fn reports_old_sessions_as_idle_and_tolerates_malformed_records() {
        let fixture = Fixture::new();
        let projects_root = fixture.path().join("projects");
        let project_dir = projects_root.join("project");
        let cwd = fixture.path().join("cwd");
        fs::create_dir_all(&project_dir).expect("project fixture");
        fs::create_dir_all(&cwd).expect("cwd fixture");
        fs::write(
            project_dir.join("session.jsonl"),
            format!(
                concat!(
                    "not-json\n",
                    "{{\"sessionId\":\"{}\",\"cwd\":{:?},",
                    "\"timestamp\":\"1970-01-01T00:00:10Z\"}}\n"
                ),
                SESSION_ID,
                cwd.to_string_lossy()
            ),
        )
        .expect("transcript fixture");

        let discovery = discover_claude_sessions_in(
            &projects_root,
            UNIX_EPOCH + Duration::from_secs(10 + 16 * 60),
            false,
        );
        assert_eq!(discovery.sessions.len(), 1);
        assert_eq!(discovery.sessions[0].state, ClaudeSessionState::Idle);
        assert!(!discovery.sessions[0].resume_available);
    }

    #[test]
    fn uses_max_timestamp_and_its_cwd_when_record_order_is_not_chronological() {
        let fixture = Fixture::new();
        let projects_root = fixture.path().join("projects");
        let project_dir = projects_root.join("project");
        let old_cwd = fixture.path().join("old-cwd");
        let newest_cwd = fixture.path().join("newest-cwd");
        let later_line_older_cwd = fixture.path().join("later-line-older-cwd");
        fs::create_dir_all(&project_dir).expect("project fixture");
        fs::create_dir_all(&old_cwd).expect("old cwd fixture");
        fs::create_dir_all(&newest_cwd).expect("newest cwd fixture");
        fs::create_dir_all(&later_line_older_cwd).expect("later line cwd fixture");

        let transcript = format!(
            concat!(
                "{{\"sessionId\":\"{id}\",\"cwd\":{old:?},",
                "\"gitBranch\":\"old\",\"timestamp\":\"1970-01-01T00:00:10Z\"}}\n",
                "{{\"sessionId\":\"{id}\",\"cwd\":{newest:?},",
                "\"gitBranch\":\"newest\",\"timestamp\":\"1970-01-01T00:00:30.500Z\"}}\n",
                "{{\"sessionId\":\"{id}\",\"cwd\":{later:?},",
                "\"gitBranch\":\"later-line\",\"timestamp\":\"1970-01-01T00:00:20Z\"}}\n"
            ),
            id = SESSION_ID,
            old = old_cwd.to_string_lossy(),
            newest = newest_cwd.to_string_lossy(),
            later = later_line_older_cwd.to_string_lossy(),
        );
        fs::write(project_dir.join("different-name.jsonl"), transcript)
            .expect("transcript fixture");

        let discovery =
            discover_claude_sessions_in(&projects_root, UNIX_EPOCH + Duration::from_secs(31), true);
        let session = &discovery.sessions[0];
        assert_eq!(session.id, SESSION_ID);
        assert_eq!(session.cwd, newest_cwd);
        assert_eq!(session.branch.as_deref(), Some("newest"));
        assert_eq!(session.last_activity_ms, 30_500);
        assert_eq!(session.state, ClaudeSessionState::Recent);
    }

    #[test]
    fn validates_session_ids_and_existing_working_directories() {
        assert!(validate_session_id(SESSION_ID).is_ok());
        assert!(validate_session_id("../../evil").is_err());
        assert!(validate_session_id("12345678-1234-4abc-8def-1234567890ab;calc").is_err());

        let fixture = Fixture::new();
        assert!(validate_working_directory(fixture.path().to_string_lossy().as_ref()).is_ok());
        assert!(validate_working_directory("relative/path").is_err());
        assert!(validate_working_directory(
            fixture.path().join("missing").to_string_lossy().as_ref()
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn terminal_arguments_keep_user_values_out_of_powershell_source() {
        use super::terminal_resume_args;

        let cwd = Path::new(r"C:\work folder\relay & safe");
        let args = terminal_resume_args(SESSION_ID, cwd);
        assert_eq!(args[2], cwd.as_os_str());
        assert_eq!(args.last(), Some(&OsString::from(SESSION_ID)));
        assert_eq!(args[7], OsString::from("& claude --resume $args[0]"));
        assert!(!args[7].to_string_lossy().contains(SESSION_ID));
        assert!(!args[7].to_string_lossy().contains("work folder"));
    }

    #[cfg(windows)]
    #[test]
    fn claude_login_uses_a_constant_visible_terminal_command() {
        let args = claude_login_args();
        assert_eq!(args[0], OsString::from("new-tab"));
        assert_eq!(args[1], OsString::from("powershell.exe"));
        assert_eq!(args[5], OsString::from("& claude auth login"));
    }
}
