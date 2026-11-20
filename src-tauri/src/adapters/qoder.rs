//! Qoder IDE and Qoder CN desktop local usage stores.
//! Only token metadata, request identifiers and session metadata are read.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const IDE_AGENT: &str = "qoder-ide";
pub const CN_AGENT: &str = "qoder-cn";

fn candidate_dirs(product: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = dirs::config_dir() {
        dirs.push(root.join(product).join("SharedClientCache").join("cache").join("db"));
    }
    if let Some(root) = dirs::data_local_dir() {
        dirs.push(root.join(product).join("SharedClientCache").join("cache").join("db"));
    }
    dirs.retain(|p| p.is_dir());
    dirs.sort();
    dirs.dedup();
    dirs
}

fn database_paths(product: &str) -> Vec<PathBuf> {
    candidate_dirs(product)
        .into_iter()
        .map(|p| p.join("local.db"))
        .filter(|p| p.is_file())
        .collect()
}

pub fn ide_data_dirs() -> Vec<PathBuf> {
    candidate_dirs("Qoder")
}

pub fn cn_data_dirs() -> Vec<PathBuf> {
    let mut dirs = candidate_dirs("QoderCN");
    dirs.extend(util::env_dirs("QODERCN_CONFIG_DIR", ".qoder-cn").into_iter().map(|p| p.join("projects"))
        .filter(|p| p.is_dir()));
    dirs.sort();
    dirs.dedup();
    dirs
}

pub fn ide_collect_files() -> Vec<PathBuf> {
    database_paths("Qoder")
}

pub fn cn_collect_files() -> Vec<PathBuf> {
    let mut paths = database_paths("QoderCN");
    paths.extend(util::env_dirs("QODERCN_CONFIG_DIR", ".qoder-cn").into_iter().map(|p| p.join("projects"))
        .filter(|p| p.is_dir()).flat_map(|p| {
            let mut files = Vec::new();
            util::collect_with_ext(&p, &["jsonl"], &mut files);
            files.retain(|f| f.extension().is_some_and(|ext| ext == "jsonl"));
            files
        }));
    // Custom-model JSONL is a separate Qoder CN source. If a database is
    // shared with the IDE adapter, avoid counting it twice under CN.
    let ide_paths = ide_collect_files();
    paths.retain(|path| !ide_paths.contains(path));
    paths.sort();
    paths.dedup();
    paths
}

pub fn ide_parse_file(path: &Path) -> Vec<UsageRecord> {
    parse_sqlite(path, IDE_AGENT)
}

pub fn cn_parse_file(path: &Path) -> Vec<UsageRecord> {
    if path.extension().is_some_and(|ext| ext == "jsonl") {
        parse_cn_jsonl(path)
    } else {
        parse_sqlite(path, CN_AGENT)
    }
}

fn lenient_u64(value: &Value, keys: &[&str]) -> u64 {
    keys.iter().map(|key| value.get(*key).map(util::as_u64).unwrap_or(0)).find(|n| *n > 0).unwrap_or(0)
}

fn valid_row(usage: &Value) -> bool {
    ["prompt_tokens", "completion_tokens", "input_tokens", "output_tokens"]
        .iter().any(|k| usage.get(*k).is_some_and(|v| util::as_u64(v) > 0))
}

fn record(agent: &str, project: String, session: String, request: &str, ts: i64, model: &str, input: u64, output: u64, cached: u64, cache_write: u64) -> Option<UsageRecord> {
    if request.trim().is_empty() || input == 0 && output == 0 && cached == 0 && cache_write == 0 {
        return None;
    }
    Some(UsageRecord {
        agent: agent.to_string(),
        project,
        session_id: session,
        timestamp_ms: ts,
        model: model.to_string(),
        input_tokens: input.saturating_sub(cached.min(input)),
        output_tokens: output,
        cache_creation_5m: cache_write,
        cache_creation_1h: 0,
        cache_read_tokens: cached.min(input),
        cost_usd: None,
        dedup_key: Some(format!("{agent}:{request}")),
    })
}

fn parse_sqlite(path: &Path, agent: &str) -> Vec<UsageRecord> {
    let Some(conn) = util::open_readonly_db(path) else { return Vec::new() };
    if !util::table_exists(&conn, "chat_message") || !util::table_exists(&conn, "chat_session") { return Vec::new() }
    let Ok(mut stmt) = conn.prepare("SELECT m.request_id, m.session_id, m.token_info, m.model_info, m.gmt_create, s.project_name, s.project_uri FROM chat_message m LEFT JOIN chat_session s ON s.session_id = m.session_id WHERE m.role='assistant' AND m.token_info IS NOT NULL") else { return Vec::new() };
    let Ok(rows) = stmt.query_map([], |row| Ok((
        row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?,
        row.get::<_, Option<String>>(3)?, row.get::<_, i64>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, Option<String>>(6)?,
    ))) else { return Vec::new() };
    rows.flatten().filter_map(|(request, session, token_info, model_info, ts, project, project_uri)| {
        let usage: Value = serde_json::from_str(&token_info).ok()?;
        if !valid_row(&usage) { return None; }
        let model: Value = serde_json::from_str(model_info.as_deref().unwrap_or("{}")).unwrap_or(Value::Null);
        let model = util::get_str(&model, &["model_key"]).unwrap_or("qoder-auto");
        record(agent, project.or(project_uri).filter(|s| !s.is_empty()).unwrap_or_else(|| "Qoder".into()), session?, request.as_deref()?, util::smart_unit_ms(ts), model,
            lenient_u64(&usage, &["prompt_tokens", "input_tokens"]), lenient_u64(&usage, &["completion_tokens", "output_tokens"]),
            lenient_u64(&usage, &["cached_tokens", "cache_read_input_tokens"]), util::get_u64(&usage, "cache_creation_input_tokens"))
    }).collect()
}

fn parse_cn_jsonl(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else { return Vec::new() };
    let fallback = util::mtime_ms(path);
    let mut out = Vec::new();
    for line in content.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else { continue };
        if row.get("type").and_then(Value::as_str) != Some("assistant") { continue }
        let message = row.get("message").unwrap_or(&row);
        let Some(usage) = message.get("usage") else { continue };
        if !valid_row(usage) { continue }
        let Some(request) = util::get_str(usage, &["request_id"]) else { continue };
        let ts = row.get("timestamp").and_then(Value::as_str).and_then(util::parse_rfc3339_ms).unwrap_or(fallback);
        let model = util::get_str(message, &["model"]).or_else(|| util::get_str(&row, &["model"])).unwrap_or("qoder-cn");
        let project = util::get_str(&row, &["cwd", "projectName"]).or_else(|| util::get_str(message, &["cwd"])).unwrap_or("Qoder CN");
        let session = util::get_str(&row, &["sessionId", "session_id"]).unwrap_or(request);
        if let Some(item) = record(CN_AGENT, project.to_string(), session.to_string(), request, ts, model,
            util::get_u64(usage,"input_tokens"), util::get_u64(usage,"output_tokens"),
            util::get_u64(usage,"cache_read_input_tokens"), util::get_u64(usage,"cache_creation_input_tokens")) { out.push(item); }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_zero_token_official_rows_but_keeps_credits_out_of_token_total() {
        let dir = util::test_dir("qoder-cn");
        let path = dir.join("session.jsonl");
        std::fs::write(&path, concat!(
            "{\"type\":\"assistant\",\"timestamp\":\"2026-11-17T10:00:00Z\",\"sessionId\":\"s1\",\"model\":\"auto\",\"message\":{\"usage\":{\"input_tokens\":0,\"output_tokens\":0,\"credits\":1}}}\n",
            "{\"type\":\"assistant\",\"timestamp\":\"2026-11-17T10:01:00Z\",\"sessionId\":\"s1\",\"model\":\"byok\",\"message\":{\"usage\":{\"input_tokens\":100,\"output_tokens\":10,\"cache_read_input_tokens\":60,\"request_id\":\"r1\"}}}\n"
        )).unwrap();
        let rows=cn_parse_file(&path);
        assert_eq!(rows.len(),1);
        assert_eq!((rows[0].input_tokens,rows[0].cache_read_tokens,rows[0].output_tokens),(40,60,10));
    }
}
