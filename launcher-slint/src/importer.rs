use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const BOT_NAMES: &[&str] = &[
    "nightbot",
    "streamelements",
    "moobot",
    "fossabot",
    "stayhydratedbot",
    "wizebot",
    "soundalerts",
    "streamlabs",
    "kofi_bot",
    "botisimo",
    "buttsbot",
    "creakbot",
    "streamraiders",
];

const USER_AGENT: &str = "TwitchLogImporter/1.0 (+https://github.com)";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRecord {
    pub normalized: bool,
    pub ts: i64,
    pub kind: String,
    #[serde(default)]
    pub msg_id: String,
    pub user: String,
    pub display: String,
    pub text: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub badges: String,
    #[serde(default)]
    pub badge_info: String,
    #[serde(default)]
    pub emotes: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub room_id: String,
}

pub struct ImportParams {
    pub channel: String,
    pub dates: Vec<NaiveDate>,
    pub video_paths: Vec<PathBuf>,
    pub start_dt: Option<DateTime<Utc>>,
    pub auto_detect: bool,
    pub records_dir: PathBuf,
    pub ffmpeg_path: String,
}

/// Decode IRC tag escaped characters
fn decode_irc_tag_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    's' => out.push(' '),
                    ':' => out.push(';'),
                    'r' => out.push('\r'),
                    'n' => out.push('\n'),
                    '\\' => out.push('\\'),
                    other => out.push(other),
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse a raw IRC string line
pub fn parse_raw_irc(raw: &str) -> Option<ChatRecord> {
    let rest = raw.trim_end_matches(['\r', '\n']);
    let (tag_part, rest) = if rest.starts_with('@') {
        let (tags, rem) = rest[1..].split_once(' ')?;
        (Some(tags), rem.trim_start())
    } else {
        (None, rest)
    };

    let (prefix, rest) = if rest.starts_with(':') {
        let (pref, rem) = rest[1..].split_once(' ')?;
        (Some(pref), rem.trim_start())
    } else {
        (None, rest)
    };

    let (command_params, text) = if let Some((cp, body)) = rest.split_once(" :") {
        (cp, body.to_string())
    } else {
        (rest, String::new())
    };

    let mut parts = command_params.split_whitespace();
    let command = parts.next()?.to_uppercase();
    if command != "PRIVMSG" && command != "USERNOTICE" && command != "CLEARCHAT" && command != "CLEARMSG" && command != "NOTICE" {
        return None;
    }

    let mut tags_map = std::collections::HashMap::new();
    if let Some(t_str) = tag_part {
        for item in t_str.split(';') {
            if let Some((k, v)) = item.split_once('=') {
                tags_map.insert(k, decode_irc_tag_value(v));
            }
        }
    }

    let mut ts = 0i64;
    if let Some(tmi) = tags_map.get("tmi-sent-ts") {
        if let Ok(v) = tmi.parse::<i64>() {
            ts = v;
        }
    }
    if ts == 0 {
        ts = chrono::Utc::now().timestamp_millis();
    }

    let display = tags_map.get("display-name").cloned().unwrap_or_default();
    let login = if let Some(pref) = prefix {
        pref.split('!').next().unwrap_or("user").to_string()
    } else if !display.is_empty() {
        display.to_lowercase()
    } else {
        "user".to_string()
    };

    let display_final = if display.is_empty() { login.clone() } else { display };

    let kind = if tags_map.contains_key("target-user-id") || command == "CLEARCHAT" {
        "ban".to_string()
    } else if tags_map.contains_key("msg-id") || command == "USERNOTICE" {
        "event".to_string()
    } else if command == "CLEARMSG" {
        "delete".to_string()
    } else {
        "message".to_string()
    };

    Some(ChatRecord {
        normalized: true,
        ts,
        kind,
        msg_id: tags_map.get("msg-id").cloned().unwrap_or_default(),
        user: login,
        display: display_final,
        text,
        color: tags_map.get("color").cloned().unwrap_or_default(),
        badges: tags_map.get("badges").cloned().unwrap_or_default(),
        badge_info: tags_map.get("badge-info").cloned().unwrap_or_default(),
        emotes: tags_map.get("emotes").cloned().unwrap_or_default(),
        id: tags_map.get("id").cloned().unwrap_or_default(),
        room_id: tags_map.get("room-id").cloned().unwrap_or_default(),
    })
}

/// Normalize bestlogs message item (JSON object, raw IRC, or plain text)
fn normalize_message_item(val: &Value) -> Option<ChatRecord> {
    if let Some(raw_str) = val.as_str() {
        if raw_str.starts_with('@') || raw_str.starts_with(':') {
            return parse_raw_irc(raw_str);
        }
        // Plain text fallback: [YYYY-MM-DD HH:MM:SS] user: message
        static PLAIN_LOG_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = PLAIN_LOG_RE.get_or_init(|| {
            regex::Regex::new(r"^\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\]\s+([^:]+):\s+(.*)$")
                .unwrap()
        });
        if let Some(caps) = re.captures(raw_str) {
            let dt_str = caps.get(1)?.as_str();
            let user = caps.get(2)?.as_str().to_string();
            let text = caps.get(3)?.as_str().to_string();
            let naive = NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M:%S").ok()?;
            let ts = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp_millis();
            return Some(ChatRecord {
                normalized: true,
                ts,
                kind: "message".to_string(),
                msg_id: String::new(),
                user: user.clone(),
                display: user,
                text,
                color: String::new(),
                badges: String::new(),
                badge_info: String::new(),
                emotes: String::new(),
                id: String::new(),
                room_id: String::new(),
            });
        }
        return None;
    }

    if let Some(obj) = val.as_object() {
        // If already normalized
        if obj.get("normalized").and_then(|v| v.as_bool()) == Some(true) {
            if let Ok(rec) = serde_json::from_value::<ChatRecord>(val.clone()) {
                return Some(rec);
            }
        }

        // Check if there is raw field
        if let Some(raw) = obj.get("raw").and_then(|r| r.as_str()) {
            if let Some(parsed) = parse_raw_irc(raw) {
                return Some(parsed);
            }
        }

        let tags = obj.get("tags").and_then(|t| t.as_object());

        // Extract timestamp
        let mut ts = 0i64;
        if let Some(t_ts) = tags.and_then(|t| t.get("tmi-sent-ts")) {
            if let Some(s) = t_ts.as_str() {
                ts = s.parse::<i64>().unwrap_or(0);
            } else if let Some(n) = t_ts.as_i64() {
                ts = n;
            }
        }
        if ts == 0 {
            if let Some(iso_str) = obj.get("timestamp").or_else(|| obj.get("time")).and_then(|v| v.as_str()) {
                if let Ok(dt) = DateTime::parse_from_rfc3339(iso_str) {
                    ts = dt.timestamp_millis();
                } else if let Ok(ndt) = NaiveDateTime::parse_from_str(iso_str, "%Y-%m-%dT%H:%M:%S%.fZ") {
                    ts = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc).timestamp_millis();
                }
            }
        }
        if ts == 0 {
            return None;
        }

        let text = obj.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();

        let display = obj
            .get("displayName")
            .or_else(|| tags.and_then(|t| t.get("display-name")))
            .or_else(|| obj.get("display"))
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();

        let user = obj
            .get("user")
            .and_then(|u| {
                if let Some(s) = u.as_str() {
                    Some(s.to_string())
                } else if let Some(uo) = u.as_object() {
                    uo.get("login").or_else(|| uo.get("name")).and_then(|n| n.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .or_else(|| {
                if !display.is_empty() {
                    Some(display.to_lowercase())
                } else {
                    None
                }
            })
            .or_else(|| obj.get("channel").and_then(|c| c.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "user".to_string());

        let display_final = if display.is_empty() { user.clone() } else { display };

        let color = tags
            .and_then(|t| t.get("color"))
            .or_else(|| obj.get("color"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let badges = tags
            .and_then(|t| t.get("badges"))
            .or_else(|| obj.get("badges"))
            .and_then(|b| b.as_str())
            .unwrap_or("")
            .to_string();

        let badge_info = tags
            .and_then(|t| t.get("badge-info").or_else(|| t.get("badge_info")))
            .or_else(|| obj.get("badge_info"))
            .and_then(|b| b.as_str())
            .unwrap_or("")
            .to_string();

        let emotes = tags
            .and_then(|t| t.get("emotes"))
            .or_else(|| obj.get("emotes"))
            .and_then(|e| e.as_str())
            .unwrap_or("")
            .to_string();

        let id = tags
            .and_then(|t| t.get("id"))
            .or_else(|| obj.get("id"))
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();

        let room_id = tags
            .and_then(|t| t.get("room-id").or_else(|| t.get("room_id")))
            .or_else(|| obj.get("room_id"))
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        let msg_id = tags
            .and_then(|t| t.get("msg-id").or_else(|| t.get("msg_id")))
            .or_else(|| obj.get("msg_id"))
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();

        let kind = if tags.and_then(|t| t.get("target-user-id")).is_some() {
            "ban".to_string()
        } else if !msg_id.is_empty() {
            "event".to_string()
        } else {
            "message".to_string()
        };

        return Some(ChatRecord {
            normalized: true,
            ts,
            kind,
            msg_id,
            user,
            display: display_final,
            text,
            color,
            badges,
            badge_info,
            emotes,
            id,
            room_id,
        });
    }

    None
}

/// Estimate stream start timestamp using message density
fn detect_stream_start_ms(messages: &[ChatRecord], target_date: Option<NaiveDate>) -> (i64, String) {
    if messages.is_empty() {
        return (Utc::now().timestamp_millis(), "Нет сообщений, использовано текущее время".to_string());
    }

    let mut candidates: Vec<&ChatRecord> = messages.iter().collect();
    candidates.sort_by_key(|m| m.ts);

    if let Some(td) = target_date {
        let filtered: Vec<&ChatRecord> = candidates
            .iter()
            .copied()
            .filter(|m| {
                if let Some(ndt) = DateTime::from_timestamp_millis(m.ts) {
                    ndt.date_naive() >= td
                } else {
                    false
                }
            })
            .collect();

        if !filtered.is_empty() {
            candidates = filtered;
        }
    }

    let human: Vec<&ChatRecord> = candidates
        .iter()
        .copied()
        .filter(|m| m.kind == "message" && !BOT_NAMES.contains(&m.user.to_lowercase().as_str()))
        .collect();

    let pool = if !human.is_empty() { &human } else { &candidates };

    // Sliding window of 120 seconds, >= 10 messages from >= 3 unique users
    const WINDOW_MS: i64 = 120_000;
    let mut window: VecDeque<&ChatRecord> = VecDeque::new();

    for msg in pool {
        window.push_back(msg);
        while let Some(front) = window.front() {
            if msg.ts - front.ts > WINDOW_MS {
                window.pop_front();
            } else {
                break;
            }
        }

        let mut unique_users = HashSet::new();
        for m in &window {
            let u = if !m.display.is_empty() {
                m.display.to_lowercase()
            } else {
                m.user.to_lowercase()
            };
            if !u.is_empty() && u != "user" {
                unique_users.insert(u);
            }
        }

        if window.len() >= 10 && unique_users.len() >= 3 {
            let start_ts = window.front().unwrap().ts;
            let start_dt = DateTime::from_timestamp_millis(start_ts).unwrap_or_else(Utc::now);
            return (
                start_ts,
                format!(
                    "Автоопределение по плотности сообщений: {} ({} сообщ. за 120с от {} польз.)",
                    start_dt.format("%Y-%m-%d %H:%M:%S UTC"),
                    window.len(),
                    unique_users.len()
                ),
            );
        }
    }

    // Fallback: first available message
    let fallback = pool.first().unwrap();
    let fallback_dt = DateTime::from_timestamp_millis(fallback.ts).unwrap_or_else(Utc::now);
    (
        fallback.ts,
        format!(
            "Первое доступное сообщение (нет всплеска плотности): {}",
            fallback_dt.format("%Y-%m-%d %H:%M:%S UTC")
        ),
    )
}

fn parse_ffmpeg_stderr_duration_ms(stderr: &str) -> Option<i64> {
    static RE_DURATION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE_DURATION.get_or_init(|| {
        regex::Regex::new(r"Duration:\s*(\d{2,}):(\d{2}):(\d{2}(?:\.\d+)?)").unwrap()
    });
    let caps = re.captures(stderr)?;
    let hours: f64 = caps.get(1)?.as_str().parse().ok()?;
    let mins: f64 = caps.get(2)?.as_str().parse().ok()?;
    let secs: f64 = caps.get(3)?.as_str().parse().ok()?;
    let total_sec = hours * 3600.0 + mins * 60.0 + secs;
    if total_sec > 0.0 && total_sec.is_finite() {
        Some((total_sec * 1000.0).round() as i64)
    } else {
        None
    }
}

/// Probe video duration in milliseconds using ffprobe (with fallback to ffmpeg -i stderr)
pub fn probe_video_duration_ms(path: &Path, ffmpeg_path: Option<&str>) -> Option<i64> {
    let mut ffprobe_candidates: Vec<PathBuf> = Vec::new();
    if let Some(fp) = ffmpeg_path.map(str::trim).filter(|s| !s.is_empty()) {
        let fp_path = Path::new(fp);
        if let Some(parent) = fp_path.parent() {
            if !parent.as_os_str().is_empty() {
                #[cfg(windows)]
                ffprobe_candidates.push(parent.join("ffprobe.exe"));
                ffprobe_candidates.push(parent.join("ffprobe"));
            }
        }
    }
    ffprobe_candidates.push(PathBuf::from("ffprobe"));

    for bin in &ffprobe_candidates {
        let mut cmd = std::process::Command::new(bin);
        cmd.args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(path);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        if let Ok(output) = cmd.output() {
            if output.status.success() {
                if let Ok(json_val) = serde_json::from_slice::<Value>(&output.stdout) {
                    if let Some(dur_str) = json_val
                        .get("format")
                        .and_then(|f| f.get("duration"))
                        .and_then(|d| d.as_str())
                    {
                        if let Ok(dur_f) = dur_str.parse::<f64>() {
                            if dur_f > 0.0 && dur_f.is_finite() {
                                return Some((dur_f * 1000.0).round() as i64);
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: parse `Duration: HH:MM:SS.xx` from `ffmpeg -i <path>` stderr
    let ffmpeg_bin = ffmpeg_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("ffmpeg");
    let mut cmd = std::process::Command::new(ffmpeg_bin);
    cmd.arg("-hide_banner").arg("-i").arg(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    if let Ok(output) = cmd.output() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if let Some(ms) = parse_ffmpeg_stderr_duration_ms(&stderr) {
            return Some(ms);
        }
    }

    None
}

pub async fn fetch_day_log(
    client: &reqwest::Client,
    channel: &str,
    year: &str,
    month: &str,
    day: &str,
) -> Result<Value, String> {
    let url = format!(
        "https://bestlogs.lurkology.com/channel/{}/{}/{}/{}?jsonBasic=1",
        channel.to_lowercase(),
        year,
        month,
        day
    );

    let res = client
        .get(&url)
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache")
        .send()
        .await
        .map_err(|e| format!("Сетевая ошибка при запросе {}: {}", url, e))?;

    if !res.status().is_success() {
        return Err(format!("Архив вернул HTTP {} для {}", res.status(), url));
    }

    res.json::<Value>()
        .await
        .map_err(|e| format!("Не удалось разобрать JSON-ответ от архива {}: {}", url, e))
}

fn extract_and_normalize_messages(day_sources: &[(String, Value)]) -> Vec<ChatRecord> {
    let mut all_messages = Vec::new();
    for (_, source) in day_sources {
        if let Some(messages) = source.get("messages").and_then(|m| m.as_array()) {
            for item in messages {
                if let Some(record) = normalize_message_item(item) {
                    all_messages.push(record);
                }
            }
        } else if let Some(arr) = source.as_array() {
            for item in arr {
                if let Some(record) = normalize_message_item(item) {
                    all_messages.push(record);
                }
            }
        }
    }
    all_messages
}

fn deduplicate_messages(messages: &mut Vec<ChatRecord>) {
    messages.sort_by_key(|m| m.ts);
    let mut seen_ids = HashSet::with_capacity(messages.len());
    let mut unique = Vec::with_capacity(messages.len());
    for msg in messages.drain(..) {
        let key = if !msg.id.is_empty() {
            format!("id:{}", msg.id)
        } else {
            format!("c:{}:{}:{}", msg.ts, msg.user, msg.text)
        };
        if seen_ids.insert(key) {
            unique.push(msg);
        }
    }
    *messages = unique;
}

/// Checks whether a stream that had messages near midnight UTC continues into the next day's log.
fn is_midnight_continuation(last_prev_day_ts: i64, first_next_day_ts: i64, next_date: NaiveDate) -> bool {
    let gap_ms = first_next_day_ts - last_prev_day_ts;
    let first_early_utc = DateTime::from_timestamp_millis(first_next_day_ts)
        .map(|dt| {
            dt.date_naive() == next_date
                && dt.time() <= chrono::NaiveTime::from_hms_opt(1, 0, 0).unwrap()
        })
        .unwrap_or(false);
    first_early_utc || (0..=45 * 60 * 1000).contains(&gap_ms)
}

pub async fn run_import_task<F: Fn(String) + Send + 'static>(
    params: ImportParams,
    log_fn: F,
) -> Result<PathBuf, String> {
    let channel = params.channel.to_lowercase();
    log_fn(format!(
        "Начало импорта для канала {}; дат: {}",
        channel,
        params.dates.len()
    ));

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Ошибка создания HTTP клиента: {}", e))?;

    let mut day_sources = Vec::new();
    let mut attempted_dates: HashSet<NaiveDate> = HashSet::new();

    for date in &params.dates {
        if !attempted_dates.insert(*date) {
            continue;
        }
        let y = date.format("%Y").to_string();
        let m = date.format("%m").to_string();
        let d = date.format("%d").to_string();
        log_fn(format!("Загрузка архива за {}...", date));

        match fetch_day_log(&client, &channel, &y, &m, &d).await {
            Ok(content) => day_sources.push((format!("{}-{}-{}", y, m, d), content)),
            Err(e) => log_fn(format!("Предупреждение: {}", e)),
        }
    }

    if day_sources.is_empty() {
        return Err("Не удалось загрузить ни одного лога за указанные даты.".to_string());
    }

    log_fn("Парсинг и нормализация сообщений чата...".to_string());
    let mut all_messages = extract_and_normalize_messages(&day_sources);

    if all_messages.is_empty() {
        return Err("В загруженных архивах не найдено ни одного сообщения.".to_string());
    }

    deduplicate_messages(&mut all_messages);
    log_fn(format!("Всего уникальных сообщений (начальная выборка): {}", all_messages.len()));

    // Probe video durations once upfront (used both for midnight span detection and for parts manifest)
    let mut total_video_duration_ms = 0i64;
    let mut probed_durations: Vec<Option<i64>> = Vec::with_capacity(params.video_paths.len());
    if !params.video_paths.is_empty() {
        log_fn(format!("Анализ видеофайлов ({}) с помощью ffprobe...", params.video_paths.len()));
        for (i, vp) in params.video_paths.iter().enumerate() {
            let canonical = vp.canonicalize().unwrap_or_else(|_| vp.clone());
            let dur = probe_video_duration_ms(&canonical, Some(&params.ffmpeg_path));
            if let Some(d) = dur {
                total_video_duration_ms += d;
                log_fn(format!(
                    "  [Видео {}] {} -> длительность {:.1} с",
                    i + 1,
                    canonical.file_name().unwrap_or_default().to_string_lossy(),
                    d as f64 / 1000.0
                ));
            } else {
                log_fn(format!(
                    "  [Видео {}] {} -> длительность не определена",
                    i + 1,
                    canonical.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            probed_durations.push(dur);
        }
    }

    // Determine initial session start
    let target_date = params.dates.first().copied();
    let (mut session_start_ms, mut detection_info) = if let Some(st) = params.start_dt {
        (st.timestamp_millis(), "Указано ручное время старта".to_string())
    } else if params.auto_detect {
        detect_stream_start_ms(&all_messages, target_date)
    } else {
        (all_messages[0].ts, "Первое сообщение архива".to_string())
    };

    let session_start_dt = DateTime::from_timestamp_millis(session_start_ms)
        .unwrap_or_else(Utc::now);

    // If video files are provided and located inside a session subfolder, reuse that folder directly!
    let (session_dir, folder_name) = if let Some(first_video) = params.video_paths.first() {
        let parent = first_video.parent();
        let is_in_subfolder = match parent {
            Some(p) => {
                let p_canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                let rec_canon = params.records_dir.canonicalize().unwrap_or_else(|_| params.records_dir.clone());
                if p_canon == rec_canon {
                    false
                } else if let Some(name_os) = p.file_name() {
                    let name = name_os.to_string_lossy().to_string();
                    let (_, dt) = crate::records::parse_channel_from_folder(&name);
                    let has_session = p.join(format!("{}.session.json", name)).is_file();
                    dt.is_some() || has_session
                } else {
                    false
                }
            }
            None => false,
        };

        if is_in_subfolder {
            let parent_dir = parent.unwrap().to_path_buf();
            let name = parent_dir.file_name().unwrap().to_string_lossy().to_string();
            log_fn(format!("Используется существующая папка видео: {}", parent_dir.display()));
            (parent_dir, name)
        } else {
            let fname = format!("{}_{}", channel, session_start_dt.format("%d-%m-%Y_%H-%M-%S"));
            (params.records_dir.join(&fname), fname)
        }
    } else {
        let fname = format!("{}_{}", channel, session_start_dt.format("%d-%m-%Y_%H-%M-%S"));
        (params.records_dir.join(&fname), fname)
    };

    fs::create_dir_all(&session_dir)
        .map_err(|e| format!("Не удалось создать директорию сессии {}: {}", session_dir.display(), e))?;

    let session_file = session_dir.join(format!("{}.session.json", folder_name));
    let chat_file = session_dir.join(format!("{}.chat.jsonl", folder_name));

    // If user did not specify start_dt and auto_detect is false, check if existing session.json has start time
    if params.start_dt.is_none() && !params.auto_detect && session_file.is_file() {
        if let Ok(content) = fs::read_to_string(&session_file) {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                if let Some(ms) = val.get("session_start_ms").and_then(|v| v.as_i64()) {
                    session_start_ms = ms;
                    detection_info = "Время старта из существующей сессии".to_string();
                }
            }
        }
    }

    // --- AUTO-DETECT NEXT DAY(S) WHEN STREAM CROSSES MIDNIGHT UTC ---
    let mut added_extra_days = false;

    // 1. Check by total video duration (session_start_ms .. session_start_ms + total_video_duration_ms)
    if total_video_duration_ms > 0 {
        let session_end_ms = session_start_ms + total_video_duration_ms;
        if let (Some(start_dt), Some(end_dt)) = (
            DateTime::from_timestamp_millis(session_start_ms),
            DateTime::from_timestamp_millis(session_end_ms),
        ) {
            let mut cur_date = start_dt.date_naive();
            let end_date = end_dt.date_naive();
            while cur_date <= end_date {
                if attempted_dates.insert(cur_date) {
                    let y = cur_date.format("%Y").to_string();
                    let m = cur_date.format("%m").to_string();
                    let d = cur_date.format("%d").to_string();
                    log_fn(format!(
                        "Видео переходит через полночь UTC -> догрузка чата за {}...",
                        cur_date
                    ));
                    match fetch_day_log(&client, &channel, &y, &m, &d).await {
                        Ok(content) => {
                            day_sources.push((format!("{}-{}-{}", y, m, d), content));
                            added_extra_days = true;
                        }
                        Err(e) => log_fn(format!("Предупреждение при догрузке {}: {}", cur_date, e)),
                    }
                }
                match cur_date.succ_opt() {
                    Some(next) => cur_date = next,
                    None => break,
                }
            }
        }
    }

    if added_extra_days {
        all_messages = extract_and_normalize_messages(&day_sources);
        deduplicate_messages(&mut all_messages);
    }

    // 2. Check by chat activity near midnight (23:30:00..23:59:59 UTC) even if ffprobe is unavailable
    for _ in 0..5 {
        let last_msg_ts = match all_messages.last() {
            Some(m) => m.ts,
            None => break,
        };
        let last_dt = match DateTime::from_timestamp_millis(last_msg_ts) {
            Some(dt) => dt,
            None => break,
        };

        let near_midnight = last_dt.time() >= chrono::NaiveTime::from_hms_opt(23, 30, 0).unwrap();
        if !near_midnight {
            break;
        }

        let next_date = match last_dt.date_naive().succ_opt() {
            Some(nd) => nd,
            None => break,
        };

        if !attempted_dates.insert(next_date) {
            break;
        }

        let y = next_date.format("%Y").to_string();
        let m = next_date.format("%m").to_string();
        let d = next_date.format("%d").to_string();
        log_fn(format!(
            "Активность чата у полуночи ({} UTC) -> проверка продолжения стрима за {}...",
            last_dt.format("%H:%M:%S"),
            next_date
        ));

        match fetch_day_log(&client, &channel, &y, &m, &d).await {
            Ok(content) => {
                let date_key = format!("{}-{}-{}", y, m, d);
                let next_msgs = extract_and_normalize_messages(&[(date_key.clone(), content.clone())]);
                if let Some(first_next) = next_msgs.iter().min_by_key(|msg| msg.ts) {
                    if is_midnight_continuation(last_msg_ts, first_next.ts, next_date) {
                        log_fn(format!(
                            "Обнаружено продолжение стрима после 00:00 UTC ({}): +{} сообщений",
                            next_date,
                            next_msgs.len()
                        ));
                        day_sources.push((date_key, content));
                        all_messages.extend(next_msgs);
                        deduplicate_messages(&mut all_messages);
                        added_extra_days = true;
                        continue;
                    } else {
                        log_fn(format!(
                            "В архиве за {} нет ночного продолжения стрима (первое сообщение слишком поздно), пропуск.",
                            next_date
                        ));
                    }
                }
                break;
            }
            Err(_) => break,
        }
    }

    // Keep day_sources sorted chronologically by YYYY-MM-DD key
    day_sources.sort_by(|a, b| a.0.cmp(&b.0));

    if added_extra_days {
        log_fn(format!(
            "Итого после объединения всех суток: {} уникальных сообщений",
            all_messages.len()
        ));
    }

    let session_start_dt = DateTime::from_timestamp_millis(session_start_ms)
        .unwrap_or_else(Utc::now);

    // Discover room_id across all merged days
    let room_id = all_messages
        .iter()
        .find(|m| !m.room_id.is_empty())
        .map(|m| m.room_id.clone());

    if let Some(rid) = &room_id {
        log_fn(format!("Определен room_id канала: {}", rid));
    }

    log_fn(format!("[Старт] {}", detection_info));

    // Build video parts manifest using already-probed durations
    let mut parts_json = Vec::new();
    let mut offsets_map = serde_json::Map::new();

    // Preserve existing offsets_ms if session.json already exists
    if session_file.is_file() {
        if let Ok(content) = fs::read_to_string(&session_file) {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                if let Some(existing_offsets) = val.get("offsets_ms").and_then(|v| v.as_object()) {
                    offsets_map = existing_offsets.clone();
                }
            }
        }
    }

    let mut cursor = session_start_ms;
    for (i, vp) in params.video_paths.iter().enumerate() {
        let canonical = vp.canonicalize().unwrap_or_else(|_| vp.clone());
        let file_str = if let Ok(rel) = vp.strip_prefix(&session_dir) {
            rel.to_string_lossy().replace('\\', "/")
        } else if let Ok(canon_vp) = vp.canonicalize() {
            if let Ok(canon_sess) = session_dir.canonicalize() {
                if let Ok(rel) = canon_vp.strip_prefix(&canon_sess) {
                    rel.to_string_lossy().replace('\\', "/")
                } else {
                    canonical.to_string_lossy().to_string()
                }
            } else {
                canonical.to_string_lossy().to_string()
            }
        } else {
            vp.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| canonical.to_string_lossy().to_string())
        };

        let duration = probed_durations.get(i).copied().flatten();
        let end_ms = duration.map(|dur| cursor + dur);

        parts_json.push(json!({
            "file": file_str,
            "start_ms": cursor,
            "end_ms": end_ms,
        }));

        offsets_map.entry(file_str).or_insert(json!(0));

        if let Some(end) = end_ms {
            cursor = end;
        }
    }

    // Ensure messages are sorted by timestamp
    all_messages.sort_by_key(|m| m.ts);
    log_fn(format!(
        "Собрано {} сообщений чата (период с {} по {})",
        all_messages.len(),
        all_messages.first().and_then(|m| DateTime::from_timestamp_millis(m.ts)).map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string()).unwrap_or_default(),
        all_messages.last().and_then(|m| DateTime::from_timestamp_millis(m.ts)).map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string()).unwrap_or_default(),
    ));

    // Write chat.jsonl
    log_fn(format!("Запись {} сообщений в {}...", all_messages.len(), chat_file.file_name().unwrap().to_string_lossy()));
    let mut chat_writer = File::create(&chat_file)
        .map_err(|e| format!("Не удалось создать chat.jsonl: {}", e))?;

    for msg in &all_messages {
        let line = serde_json::to_string(msg)
            .map_err(|e| format!("Ошибка сериализации сообщения: {}", e))?;
        writeln!(chat_writer, "{}", line)
            .map_err(|e| format!("Ошибка записи в chat.jsonl: {}", e))?;
    }

    // Write day source dumps for reprocessing
    let mut source_files = Vec::new();
    for (i, (_, source)) in day_sources.iter().enumerate() {
        let source_fn = format!("{}.day_{}.source.json", folder_name, i + 1);
        let source_path = session_dir.join(&source_fn);
        if let Ok(content) = serde_json::to_string_pretty(source) {
            let _ = fs::write(&source_path, content);
            source_files.push(source_fn);
        }
    }

    // Write session.json manifest
    let manifest = json!({
        "channel": channel,
        "room_id": room_id,
        "session_start_ms": session_start_ms,
        "start_time_iso": session_start_dt.to_rfc3339(),
        "start_detection_info": detection_info,
        "target_date": target_date.map(|d| d.format("%Y-%m-%d").to_string()),
        "timezone": "UTC",
        "parts": parts_json,
        "offsets_ms": Value::Object(offsets_map),
        "emote_cache_dir": "emote_cache",
        "source_files": source_files,
        "imported_message_count": all_messages.len(),
        "created_at": Utc::now().to_rfc3339(),
    });

    let manifest_str = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Ошибка сериализации session.json: {}", e))?;
    fs::write(&session_file, manifest_str)
        .map_err(|e| format!("Не удалось сохранить session.json: {}", e))?;

    log_fn(format!(
        "Импорт успешно завершен! Сессия: {}, сообщений: {}",
        folder_name,
        all_messages.len()
    ));

    Ok(session_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ffmpeg_stderr_duration() {
        let sample = "  Duration: 08:15:30.50, start: 0.000000, bitrate: 6123 kb/s";
        assert_eq!(parse_ffmpeg_stderr_duration_ms(sample), Some(29_730_500));
    }

    #[test]
    fn test_deduplicate_and_midnight_continuation() {
        let d1 = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let ts_23_59 = d1.and_hms_opt(23, 59, 50).unwrap().and_utc().timestamp_millis();
        let ts_00_00 = d2.and_hms_opt(0, 0, 12).unwrap().and_utc().timestamp_millis();
        let ts_14_00 = d2.and_hms_opt(14, 0, 0).unwrap().and_utc().timestamp_millis();

        assert!(is_midnight_continuation(ts_23_59, ts_00_00, d2));
        assert!(!is_midnight_continuation(ts_23_59, ts_14_00, d2));

        let day1_json = json!({
            "messages": [
                { "id": "m1", "timestamp": "2026-11-13T23:59:50Z", "user": "alice", "text": "before midnight" },
                { "id": "m2", "timestamp": "2026-11-14T00:00:00Z", "user": "bob", "text": "midnight boundary" }
            ]
        });
        let day2_json = json!({
            "messages": [
                { "id": "m2", "timestamp": "2026-11-14T00:00:00Z", "user": "bob", "text": "midnight boundary" },
                { "id": "m3", "timestamp": "2026-11-14T00:00:12Z", "user": "charlie", "text": "after midnight" }
            ]
        });

        let mut msgs = extract_and_normalize_messages(&[
            ("2026-11-13".to_string(), day1_json),
            ("2026-11-14".to_string(), day2_json),
        ]);
        deduplicate_messages(&mut msgs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].id, "m1");
        assert_eq!(msgs[1].id, "m2");
        assert_eq!(msgs[2].id, "m3");
    }
}
