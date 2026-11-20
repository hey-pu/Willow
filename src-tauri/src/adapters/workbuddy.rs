//! WorkBuddy local session JSONL. Conversation text is never copied to records.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "workbuddy";

pub fn data_dirs() -> Vec<PathBuf> {
    let mut paths: Vec<_> = ["WORKBUDDY_HOME", "WORKBUDDY_AI_HOME"]
        .into_iter()
        .flat_map(|key| util::env_dirs(key, ".workbuddy"))
        .map(|p| p.join("projects"))
        .filter(|p| p.is_dir())
        .collect();
    paths.sort(); paths.dedup(); paths
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files=Vec::new();
    for path in data_dirs() { util::collect_with_ext(&path,&["jsonl"],&mut files); }
    files.sort(); files.dedup(); files
}

pub fn parse_file(path:&Path)->Vec<UsageRecord>{
    let Ok(content)=std::fs::read_to_string(path) else {return Vec::new()};
    let fallback=util::mtime_ms(path);
    let mut records=Vec::new();
    for line in content.lines(){
        let Ok(row)=serde_json::from_str::<Value>(line) else{continue};
        // WorkBuddy stores assistant turns as type=message, role=assistant.
        // Some older exports use type=assistant directly.
        let is_assistant = row.get("role").and_then(Value::as_str) == Some("assistant")
            || row.get("type").and_then(Value::as_str) == Some("assistant");
        if !is_assistant {continue}
        let message=row.get("message").unwrap_or(&row);
        let Some(usage)=message.get("usage") else{continue};
        // Normalize the canonical snake_case message. Provider metadata mirrors it.
        let input=util::get_u64(usage,"input_tokens");
        let output=util::get_u64(usage,"output_tokens");
        if input==0 && output==0 {continue}
        let session=util::get_str(message,&["sessionId","session_id"])
            .or_else(||util::get_str(&row,&["sessionId","session_id"]));
        let Some(session)=session else{continue};
        let id=util::get_str(message,&["id","message_id"])
            .or_else(||util::get_str(&row,&["id","message_id"]));
        let Some(id)=id else{continue};
        let ts=row.get("timestamp").and_then(|v|util::ts_from_value(v))
            .or_else(||message.get("timestamp").and_then(|v|util::ts_from_value(v)))
            .unwrap_or(fallback);
        let cwd=util::get_str(message,&["cwd"]).or_else(||util::get_str(&row,&["cwd"]));
        let project=cwd.and_then(crate::adapters::jsonl::project_from_cwd).unwrap_or_else(||"WorkBuddy".into());
        let model=util::get_str(message,&["model"]).or_else(||util::get_str(&row,&["model"])).unwrap_or("unknown");
        let cached=util::get_u64(usage,"cache_read_input_tokens").min(input);
        let cache_write=util::get_u64(usage,"cache_creation_input_tokens");
        records.push(UsageRecord{
            agent:AGENT.to_string(),project,session_id:session.to_string(),timestamp_ms:ts,model:model.to_string(),
            input_tokens:input.saturating_sub(cached),output_tokens:output,cache_creation_5m:cache_write,
            cache_creation_1h:0,cache_read_tokens:cached,cost_usd:None,dedup_key:Some(format!("workbuddy:{session}:{id}")),
        });
    }
    records
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn reads_real_tokens_once_without_using_provider_copies(){
        let path=util::test_dir("workbuddy").join("session.jsonl");
        std::fs::write(&path,concat!(
            "{\"type\":\"message\",\"role\":\"assistant\",\"id\":\"u0\",\"message\":{\"id\":\"m1\",\"sessionId\":\"s1\",\"model\":\"provider-model\",\"usage\":{\"input_tokens\":100,\"output_tokens\":12,\"cache_read_input_tokens\":25,\"total_tokens\":112}},\"providerData\":{\"usage\":{\"inputTokens\":999,\"outputTokens\":999}}}\n",
            "{\"type\":\"function_call\",\"providerData\":{\"usage\":{\"inputTokens\":800}}}\n",
            "{\"type\":\"message\",\"role\":\"assistant\",\"id\":\"m0\",\"message\":{\"id\":\"m0\",\"sessionId\":\"s1\",\"usage\":{\"input_tokens\":0,\"output_tokens\":0}}}\n"
        )).unwrap();
        let rows=parse_file(&path);assert_eq!(rows.len(),1);assert_eq!((rows[0].input_tokens,rows[0].output_tokens,rows[0].cache_read_tokens),(75,12,25));assert_eq!(rows[0].dedup_key.as_deref(),Some("workbuddy:s1:m1"));
    }
}
