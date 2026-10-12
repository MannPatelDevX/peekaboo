//! Index of Claude Code transcripts in `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`.
use crate::model::project_name;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub const MAX_AGE_SECS: u64 = 30 * 86_400;
pub const MAX_SESSIONS: usize = 200;
const DEFAULT_WINDOW: u64 = 200_000;
const LARGE_WINDOW: u64 = 1_000_000;
const TITLE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSession {
    pub id: String,
    pub cwd: Option<String>,
    pub project: String,
    pub branch: Option<String>,
    pub title: String,
    /// Seconds since the epoch the transcript was last written.
    pub last_activity: u64,
    pub model: Option<String>,
    pub context_tokens: Option<u64>,
    pub context_window: u64,
}

pub fn claude_projects_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".claude").join("projects")
}

fn truncate(s: &str) -> String {
    let s = s.trim().replace('\n', " ");
    if s.chars().count() <= TITLE_CHARS {
        s
    } else {
        format!("{}…", s.chars().take(TITLE_CHARS).collect::<String>())
    }
}

/// The user's typed prompt, skipping meta records, tool results and command wrappers like `<command-name>`.
fn prompt_text(r: &Value) -> Option<String> {
    if r.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let text = match r.pointer("/message/content")? {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .find(|i| i.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|i| i.get("text").and_then(Value::as_str))?
            .to_string(),
        _ => return None,
    };
    let t = text.trim();
    (!t.is_empty() && !t.starts_with('<')).then(|| truncate(t))
}

fn mtime_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parses one transcript. Unparseable lines are skipped; a file with no usable records yields None.
pub fn parse_transcript(path: &Path) -> Option<TranscriptSession> {
    let meta = std::fs::metadata(path).ok()?;
    let file = std::fs::File::open(path).ok()?;
    let (mut cwd, mut branch, mut model) = (None, None, None);
    let (mut custom, mut ai, mut first_prompt) = (None, None, None);
    let (mut tokens, mut large) = (None, false);
    let mut seen = false;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let Ok(r) = serde_json::from_str::<Value>(&line) else { continue };
        seen = true;
        let s = |k: &str| r.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
        if let Some(c) = s("cwd") {
            cwd = Some(c);
        }
        if let Some(b) = s("gitBranch") {
            branch = Some(b);
        }
        match r.get("type").and_then(Value::as_str) {
            Some("custom-title") => custom = s("customTitle").or(custom),
            Some("ai-title") => ai = s("aiTitle").or(ai),
            Some("user") if first_prompt.is_none() => first_prompt = prompt_text(&r),
            Some("assistant") if r.get("isSidechain").and_then(Value::as_bool) != Some(true) => {
                if let Some(u) = r.pointer("/message/usage") {
                    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    tokens = Some(n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens"));
                }
                if let Some(m) = r.pointer("/message/model").and_then(Value::as_str) {
                    model = Some(m.to_string());
                }
                large |= s("requestedModel").is_some_and(|m| m.contains("[1m]"));
            }
            _ => {}
        }
    }
    if !seen {
        return None;
    }
    let large = large || tokens.is_some_and(|t| t > DEFAULT_WINDOW);
    Some(TranscriptSession {
        id: path.file_stem()?.to_string_lossy().into_owned(),
        project: project_name(cwd.as_deref()),
        cwd,
        branch,
        title: custom.or(ai).map(|t| truncate(&t)).or(first_prompt).unwrap_or_else(|| "Untitled session".into()),
        last_activity: mtime_secs(&meta),
        model,
        context_tokens: tokens,
        context_window: if large { LARGE_WINDOW } else { DEFAULT_WINDOW },
    })
}

/// Caches parsed transcripts by (mtime, size) so rescans only read files that changed.
#[derive(Default)]
pub struct SessionIndex {
    cache: HashMap<PathBuf, ((u64, u64), Option<TranscriptSession>)>,
    /// Number of files actually parsed (for tests and diagnostics).
    pub parses: usize,
}

impl SessionIndex {
    /// Title of one session, for the live Agents list. Uses any cached parse (titles rarely
    /// change, transcripts grow constantly); parses the file once if it was never seen.
    pub fn title_for(&mut self, root: &Path, id: &str) -> Option<String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        let file = format!("{id}.jsonl");
        if let Some((_, s)) = self.cache.iter().find(|(p, _)| p.file_name().is_some_and(|n| n == file.as_str())).map(|(_, v)| v) {
            return s.as_ref().map(|s| s.title.clone());
        }
        let path = std::fs::read_dir(root).ok()?.flatten().map(|d| d.path().join(&file)).find(|p| p.is_file())?;
        let meta = std::fs::metadata(&path).ok()?;
        self.parses += 1;
        let s = parse_transcript(&path);
        let title = s.as_ref().map(|s| s.title.clone());
        self.cache.insert(path, ((mtime_secs(&meta), meta.len()), s));
        title
    }

    pub fn scan(&mut self, root: &Path, now: u64) -> Vec<TranscriptSession> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let Ok(projects) = std::fs::read_dir(root) else { return out };
        for project in projects.flatten() {
            let Ok(files) = std::fs::read_dir(project.path()) else { continue };
            for f in files.flatten() {
                let path = f.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(meta) = f.metadata() else { continue };
                let key = (mtime_secs(&meta), meta.len());
                if now.saturating_sub(key.0) > MAX_AGE_SECS {
                    continue;
                }
                seen.insert(path.clone());
                let cached = self.cache.get(&path).filter(|(k, _)| *k == key).map(|(_, s)| s.clone());
                let session = match cached {
                    Some(s) => s,
                    None => {
                        self.parses += 1;
                        let s = parse_transcript(&path);
                        self.cache.insert(path.clone(), (key, s.clone()));
                        s
                    }
                };
                out.extend(session);
            }
        }
        self.cache.retain(|p, _| seen.contains(p));
        out.sort_by(|a, b| b.last_activity.cmp(&a.last_activity).then_with(|| a.id.cmp(&b.id)));
        out.truncate(MAX_SESSIONS);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn write_session(dir: &Path, id: &str, records: &[serde_json::Value]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{id}.jsonl"));
        let mut f = std::fs::File::create(&p).unwrap();
        for r in records {
            writeln!(f, "{r}").unwrap();
        }
        p
    }

    fn user(text: &str) -> serde_json::Value {
        json!({"type":"user","cwd":"/w/shop","gitBranch":"main","message":{"role":"user","content":text}})
    }

    fn assistant(input: u64, cache_read: u64, sidechain: bool, requested: &str) -> serde_json::Value {
        json!({"type":"assistant","isSidechain":sidechain,"cwd":"/w/shop","gitBranch":"feat/x","requestedModel":requested,
            "message":{"model":"claude-opus-5-5","usage":{"input_tokens":input,"cache_read_input_tokens":cache_read,"cache_creation_input_tokens":10,"output_tokens":5}}})
    }

    #[test]
    fn parses_core_fields() {
        let d = tempfile::tempdir().unwrap();
        let p = write_session(d.path(), "abc", &[
            json!({"type":"user","isMeta":true,"message":{"content":"<local-command-caveat>ignore</local-command-caveat>"}}),
            user("<command-name>/clear</command-name>"),
            user("Fix the checkout bug"),
            assistant(1000, 4000, false, "claude-opus-5-5"),
            assistant(999_999, 0, true, "claude-opus-5-5"),
        ]);
        let s = parse_transcript(&p).unwrap();
        assert_eq!(s.id, "abc");
        assert_eq!(s.cwd.as_deref(), Some("/w/shop"));
        assert_eq!(s.project, "shop");
        assert_eq!(s.branch.as_deref(), Some("feat/x"));
        assert_eq!(s.title, "Fix the checkout bug");
        assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(s.context_tokens, Some(5010)); // sidechain usage ignored
        assert_eq!(s.context_window, 200_000);
    }

    #[test]
    fn title_prefers_custom_then_latest_ai_title() {
        let d = tempfile::tempdir().unwrap();
        let p = write_session(d.path(), "t1", &[
            user("first prompt"),
            json!({"type":"ai-title","aiTitle":"Old title"}),
            json!({"type":"ai-title","aiTitle":"Checkout bug fix"}),
        ]);
        assert_eq!(parse_transcript(&p).unwrap().title, "Checkout bug fix");
        let p = write_session(d.path(), "t2", &[
            json!({"type":"ai-title","aiTitle":"AI"}),
            json!({"type":"custom-title","customTitle":"Mine"}),
        ]);
        assert_eq!(parse_transcript(&p).unwrap().title, "Mine");
    }

    #[test]
    fn list_content_and_long_prompts() {
        let d = tempfile::tempdir().unwrap();
        let p = write_session(d.path(), "l", &[json!({"type":"user","message":{"content":[
            {"type":"tool_result","content":"x"}, {"type":"text","text": "y".repeat(300)}]}})]);
        let s = parse_transcript(&p).unwrap();
        assert!(s.title.starts_with("yyy") && s.title.chars().count() <= 81);
    }

    #[test]
    fn one_million_window_detected() {
        let d = tempfile::tempdir().unwrap();
        let p = write_session(d.path(), "m", &[assistant(10, 0, false, "claude-opus-5-5[1m]")]);
        assert_eq!(parse_transcript(&p).unwrap().context_window, 1_000_000);
        let p = write_session(d.path(), "n", &[assistant(250_000, 0, false, "claude-opus-5-5")]);
        assert_eq!(parse_transcript(&p).unwrap().context_window, 1_000_000);
    }

    #[test]
    fn malformed_lines_tolerated_and_empty_files_skipped() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad.jsonl");
        std::fs::write(&p, "{not json\n{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n").unwrap();
        assert_eq!(parse_transcript(&p).unwrap().title, "hello");
        let e = d.path().join("empty.jsonl");
        std::fs::write(&e, "").unwrap();
        assert!(parse_transcript(&e).is_none());
    }

    #[test]
    fn scan_filters_old_sorts_newest_and_caches() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("-w-shop");
        let a = write_session(&proj, "a", &[user("a")]);
        let b = write_session(&proj, "b", &[user("b")]);
        let old = write_session(&proj, "old", &[user("old")]);
        let set = |p: &Path, secs: u64| {
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
            std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
        };
        let now = 100 * 86_400;
        set(&a, now - 100);
        set(&b, now - 50);
        set(&old, now - 31 * 86_400);
        let mut idx = SessionIndex::default();
        let ids: Vec<_> = idx.scan(d.path(), now).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["b", "a"]);
        assert_eq!(idx.parses, 2);
        idx.scan(d.path(), now);
        assert_eq!(idx.parses, 2, "unchanged files are not re-parsed");
        write_session(&proj, "a", &[user("a"), user("more")]);
        set(&a, now - 10);
        let first = idx.scan(d.path(), now);
        assert_eq!(idx.parses, 3);
        assert_eq!(first[0].id, "a");
    }

    #[test]
    fn missing_root_is_empty() {
        let mut idx = SessionIndex::default();
        assert!(idx.scan(Path::new("/definitely/not/here"), 0).is_empty());
    }

    #[test]
    fn title_lookup_parses_once_then_uses_cache() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("-w-shop");
        write_session(&proj, "live", &[json!({"type":"ai-title","aiTitle":"Checkout fix"})]);
        let mut idx = SessionIndex::default();
        assert_eq!(idx.title_for(d.path(), "live").as_deref(), Some("Checkout fix"));
        write_session(&proj, "live", &[json!({"type":"ai-title","aiTitle":"Checkout fix"}), user("more")]);
        assert_eq!(idx.title_for(d.path(), "live").as_deref(), Some("Checkout fix"));
        assert_eq!(idx.parses, 1, "growing transcripts aren't re-read for a title");
        assert_eq!(idx.title_for(d.path(), "missing"), None);
        assert_eq!(idx.title_for(d.path(), "../etc"), None);
    }
}
