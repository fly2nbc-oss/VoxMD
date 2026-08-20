use std::time::Duration;

use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
};
use async_openai::Client;

use crate::config::{resolve_summary_language, AppConfig};

/// The summary is fact extraction; a fixed low temperature beats a user setting here.
const SUMMARY_TEMPERATURE: f32 = 0.3;
/// The summary is capped at ~600 words; this leaves generous headroom.
const SUMMARY_MAX_TOKENS: u32 = 8192;
/// Transcript input is truncated to keep the request within typical context limits.
const SUMMARY_MAX_INPUT_CHARS: usize = 50_000;

/// Prompts are authored in English (keeps timestamps ASCII); the output language is enforced.
fn summary_system_prompt(lang: &str) -> String {
    format!(
        r###"You summarize the transcript of an audio recording.

Language: Write the ENTIRE output in "{lang}" (ISO 639-1) — every heading, bullet point, and sentence. Translate the section headings below into that language, keeping their order and meaning.

Rules:
1. Start directly with the first "##" heading. No preamble, no commentary, no code fences.
2. Focus on substance: key statements, arguments, numbers. Ignore small talk, advertising, and filler.
3. Use exactly this outline:
   ## Summary in One Sentence — the core topic in one concise sentence.
   ## Key Arguments & Insights — up to 10 bullet points with the core statements.
   ## Data & Facts — significant numbers, statistics, and dates that are mentioned. Omit this section entirely if there are none.
   ## Notable Quotes — 3 to 8 short verbatim quotes carrying concrete facts or striking statements. One per line, formatted as: > "verbatim quote" [HH:MM:SS] — using the timestamp of the transcript line the quote starts on. Omit this section entirely if nothing stands out.
4. Style: factual, concise, informative. No filler words. Do not state anything that is not in the transcript.
5. Length: at most about 600 words in total."###
    )
}

/// Upper bound for one HTTP attempt. async-openai 0.41 still retries 429/5xx
/// (OpenAIRetryLayer, default max 3 retries, backoff capped at 8s). The timeout
/// applies per attempt via the injected reqwest client; cancel is only checked
/// before `generate_summary`, not during backoff.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

pub fn make_client(cfg: &AppConfig) -> Client<OpenAIConfig> {
    let oc = OpenAIConfig::new()
        .with_api_base(cfg.api_base_url.trim_end_matches('/').to_string())
        .with_api_key(cfg.api_key.clone());

    let http = reqwest::Client::builder()
        .user_agent(crate::podcast::USER_AGENT)
        .connect_timeout(Duration::from_secs(30))
        .timeout(REQUEST_TIMEOUT)
        .build();

    match http {
        Ok(http) => Client::with_config(oc).with_http_client(http),
        // Falling back to the default client keeps the summary working; it just
        // loses the timeout, which is strictly better than failing the batch.
        Err(_) => Client::with_config(oc),
    }
}

/// Whether the transcript exceeds the summary input cap (byte length).
pub fn transcript_truncated_for_summary(transcript: &str) -> bool {
    transcript.len() > SUMMARY_MAX_INPUT_CHARS
}

async fn call_llm(
    client: &Client<OpenAIConfig>,
    model: &str,
    temperature: f32,
    max_tokens: u32,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let sys = ChatCompletionRequestSystemMessageArgs::default()
        .content(system.to_string())
        .build()
        .map_err(|e| e.to_string())?;

    let usr = ChatCompletionRequestUserMessageArgs::default()
        .content(user.to_string())
        .build()
        .map_err(|e| e.to_string())?;

    let req = CreateChatCompletionRequestArgs::default()
        .model(model)
        .messages(vec![
            ChatCompletionRequestMessage::System(sys),
            ChatCompletionRequestMessage::User(usr),
        ])
        .temperature(temperature)
        .max_tokens(max_tokens)
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.chat().create(req).await.map_err(|e| e.to_string())?;

    let text = resp
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default()
        .trim()
        .to_string();

    // An empty completion (no choices, a content filter, or a tool-only reply) is
    // a failure, not a successful empty summary — the caller would otherwise write
    // a Markdown file with nothing but its heading.
    if text.is_empty() {
        return Err("the model returned an empty response".to_string());
    }

    Ok(text)
}

/// `context` is a short orientation block (title, podcast/episode info); may be empty.
pub async fn generate_summary(
    client: &Client<OpenAIConfig>,
    cfg: &AppConfig,
    context: &str,
    transcript: &str,
) -> Result<String, String> {
    let text_for_summary = if transcript_truncated_for_summary(transcript) {
        let mut s = transcript
            .chars()
            .take(SUMMARY_MAX_INPUT_CHARS)
            .collect::<String>();
        s.push_str("\n\n[... transcript truncated for summary ...]");
        s
    } else {
        transcript.to_string()
    };

    let mut user = String::new();
    if !context.trim().is_empty() {
        user.push_str(
            "Recording context (orientation only — summarize the transcript, not this block):\n",
        );
        user.push_str(context.trim());
        user.push_str("\n\n");
    }
    user.push_str("Transcript:\n\n");
    user.push_str(&text_for_summary);

    let lang = resolve_summary_language(&cfg.summary_language);
    let system = summary_system_prompt(&lang);
    call_llm(
        client,
        &cfg.api_model,
        SUMMARY_TEMPERATURE,
        SUMMARY_MAX_TOKENS,
        &system,
        &user,
    )
    .await
}

pub fn fmt_ts(seconds: f32) -> String {
    let total = seconds.max(0.0) as i64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("[{h:02}:{m:02}:{s:02}]")
}

/// One Whisper segment with timestamps in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptLine {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

pub fn lines_from_state(state: &whisper_rs::WhisperState) -> Result<Vec<TranscriptLine>, String> {
    let n = state.full_n_segments();
    let mut lines = Vec::new();
    for i in 0..n {
        let Some(seg) = state.get_segment(i) else {
            continue;
        };
        let start = seg.start_timestamp() as f32 / 100.0;
        let end = seg.end_timestamp() as f32 / 100.0;
        let text = seg.to_str_lossy().unwrap_or_default().trim().to_string();
        if text.is_empty() {
            continue;
        }
        lines.push(TranscriptLine { start, end, text });
    }
    Ok(lines)
}

/// Overlap in seconds between `[a0, a1]` and `[b0, b1]`.
pub fn interval_overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// `speakers[i]` is a 1-based speaker index for `lines[i]`, or `None` to leave unlabeled.
pub fn format_transcript(lines: &[TranscriptLine], speakers: &[Option<usize>]) -> String {
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let speaker = speakers.get(i).copied().flatten();
        match speaker {
            Some(n) if n > 0 => out.push(format!(
                "{} **Speaker {n}:** {}",
                fmt_ts(line.start),
                line.text
            )),
            _ => out.push(format!("{} {}", fmt_ts(line.start), line.text)),
        }
    }
    out.join("\n")
}

/// Assign each Whisper line the speaker whose turn overlaps it the most.
pub fn speakers_for_lines(
    lines: &[TranscriptLine],
    turns: &[(f32, f32, usize)],
) -> Vec<Option<usize>> {
    lines
        .iter()
        .map(|line| {
            let mut best: Option<(f32, usize)> = None;
            for &(start, end, speaker) in turns {
                let ov = interval_overlap(line.start, line.end, start, end);
                if ov <= 0.0 {
                    continue;
                }
                if best.is_none_or(|(best_ov, _)| ov > best_ov) {
                    best = Some((ov, speaker));
                }
            }
            best.map(|(_, n)| n)
        })
        .collect()
}

fn improve_system_prompt() -> &'static str {
    "You are an expert editor for speech-to-text transcripts. Correct transcription errors, punctuation, grammar and sentence structure. Check every sentence for completeness and logic and fix problems. Rephrase slightly where needed for clarity, but preserve the meaning, the tone and the ORIGINAL LANGUAGE of the text. Return ONLY the corrected text, without comments, explanations or markdown."
}

fn translate_system_prompt(target: &str) -> String {
    format!(
        "You are a professional translator. Translate the user's text into {target}. Preserve meaning, tone and formatting. Return ONLY the translation, without comments or explanations."
    )
}

pub async fn improve_text(cfg: &AppConfig, text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("No text to improve.".to_string());
    }
    if cfg.api_key.trim().is_empty() {
        return Err("API key missing.".to_string());
    }
    let client = make_client(cfg);
    call_llm(
        &client,
        &cfg.api_model,
        SUMMARY_TEMPERATURE,
        SUMMARY_MAX_TOKENS,
        improve_system_prompt(),
        trimmed,
    )
    .await
}

pub async fn translate_text(cfg: &AppConfig, text: &str, target: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("No text to translate.".to_string());
    }
    let target = target.trim();
    if target.is_empty() {
        return Err("Target language missing.".to_string());
    }
    if cfg.api_key.trim().is_empty() {
        return Err("API key missing.".to_string());
    }
    let client = make_client(cfg);
    let system = translate_system_prompt(target);
    call_llm(
        &client,
        &cfg.api_model,
        SUMMARY_TEMPERATURE,
        SUMMARY_MAX_TOKENS,
        &system,
        trimmed,
    )
    .await
}

fn api_base(cfg: &AppConfig) -> String {
    cfg.api_base_url.trim().trim_end_matches('/').to_string()
}

fn llm_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(crate::podcast::USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))
}

fn models_url(cfg: &AppConfig) -> String {
    format!("{}/models", api_base(cfg))
}

pub async fn verify_api_key(cfg: &AppConfig) -> Result<(), String> {
    let key = cfg.api_key.trim();
    if key.is_empty() {
        return Err("API key missing.".to_string());
    }
    let base = api_base(cfg);
    if base.is_empty() {
        return Err("API base URL missing.".to_string());
    }
    let client = llm_http_client()?;
    let resp = client
        .get(models_url(cfg))
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://github.com/fly2nbc-oss/VoxMD")
        .header("X-Title", "VoxMD")
        .send()
        .await
        .map_err(|e| format!("Could not reach the API: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Key rejected (HTTP {}).", resp.status().as_u16()))
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelInfo {
    pub id: String,
}

pub async fn list_llm_models(cfg: &AppConfig) -> Result<Vec<LlmModelInfo>, String> {
    let key = cfg.api_key.trim();
    if key.is_empty() {
        return Ok(Vec::new());
    }
    let base = api_base(cfg);
    if base.is_empty() {
        return Ok(Vec::new());
    }
    let client = llm_http_client()?;
    let resp = client
        .get(models_url(cfg))
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://github.com/fly2nbc-oss/VoxMD")
        .header("X-Title", "VoxMD")
        .send()
        .await
        .map_err(|e| format!("Could not list models: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Model list failed (HTTP {}).",
            resp.status().as_u16()
        ));
    }
    let body = resp.text().await.map_err(|e| format!("Read body: {e}"))?;
    Ok(parse_model_ids(&body))
}

fn parse_model_ids(body: &str) -> Vec<LlmModelInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.as_array());
    let Some(arr) = arr else {
        return Vec::new();
    };
    let skip = regex_skip_model();
    let mut ids: Vec<String> = arr
        .iter()
        .filter_map(|m| m.get("id").and_then(|id| id.as_str()))
        .filter(|id| !skip(id))
        .map(|id| id.to_string())
        .collect();
    ids.sort();
    ids.dedup();
    ids.into_iter().map(|id| LlmModelInfo { id }).collect()
}

fn regex_skip_model() -> impl Fn(&str) -> bool {
    |id: &str| {
        let l = id.to_ascii_lowercase();
        l.contains("embed")
            || l.contains("whisper")
            || l.contains("tts")
            || l.contains("moderation")
            || l.contains("rerank")
            || l.contains("image")
            || l.contains("video")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fmt_ts, format_transcript, interval_overlap, parse_model_ids, speakers_for_lines,
        summary_system_prompt, transcript_truncated_for_summary, TranscriptLine,
        SUMMARY_MAX_INPUT_CHARS,
    };

    #[test]
    fn fmt_ts_formats_hours_minutes_seconds() {
        assert_eq!(fmt_ts(0.0), "[00:00:00]");
        assert_eq!(fmt_ts(61.4), "[00:01:01]");
        assert_eq!(fmt_ts(3723.0), "[01:02:03]");
        assert_eq!(fmt_ts(-5.0), "[00:00:00]");
    }

    #[test]
    fn summary_prompt_contains_language() {
        let p = summary_system_prompt("de");
        assert!(p.contains("\"de\""));
        assert!(p.starts_with("You summarize"));
    }

    #[test]
    fn truncation_flag_uses_byte_cap() {
        assert!(!transcript_truncated_for_summary("short"));
        let over = "a".repeat(SUMMARY_MAX_INPUT_CHARS + 1);
        assert!(transcript_truncated_for_summary(&over));
    }

    #[test]
    fn format_transcript_labels_speakers() {
        let lines = vec![
            TranscriptLine {
                start: 0.0,
                end: 1.0,
                text: "hello".into(),
            },
            TranscriptLine {
                start: 1.0,
                end: 2.0,
                text: "there".into(),
            },
        ];
        let labeled = format_transcript(&lines, &[Some(1), Some(2)]);
        assert_eq!(
            labeled,
            "[00:00:00] **Speaker 1:** hello\n[00:00:01] **Speaker 2:** there"
        );
        let raw = format_transcript(&lines, &[]);
        assert_eq!(raw, "[00:00:00] hello\n[00:00:01] there");
    }

    #[test]
    fn speaker_assignment_uses_largest_overlap() {
        let lines = [TranscriptLine {
            start: 1.0,
            end: 3.0,
            text: "hi".into(),
        }];
        let turns = [(0.0, 1.5, 1), (1.4, 4.0, 2)];
        let assigned = speakers_for_lines(&lines, &turns);
        assert_eq!(assigned, vec![Some(2)]);
        assert!(interval_overlap(0.0, 1.0, 2.0, 3.0) == 0.0);
    }

    #[test]
    fn parse_openrouter_model_list_skips_embeddings() {
        let body = r#"{"data":[{"id":"google/gemini-2.5-flash"},{"id":"openai/text-embedding-3-small"},{"id":"anthropic/claude-sonnet-4"}]}"#;
        let ids: Vec<_> = parse_model_ids(body).into_iter().map(|m| m.id).collect();
        assert!(ids.contains(&"google/gemini-2.5-flash".to_string()));
        assert!(ids.contains(&"anthropic/claude-sonnet-4".to_string()));
        assert!(!ids.iter().any(|id| id.contains("embed")));
    }
}
