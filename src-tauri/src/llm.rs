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
/// Improve/translate return the whole text, so the budget has to scale with the
/// input instead of using the summary's fixed cap. ~4 chars per token, doubled
/// because a translation can be considerably longer than its source.
const REWRITE_MIN_TOKENS: u32 = 2048;
const REWRITE_MAX_TOKENS: u32 = 32_768;
/// One LLM call is enough below this byte length (~1.7 h of speech, ~43k tokens).
const SUMMARY_SINGLE_CALL_MAX_CHARS: usize = 120_000;
/// Floor for a map-reduce part; actual part size is `max(this, total / MAX_PARTS)`.
const SUMMARY_MIN_PART_CHARS: usize = 40_000;
const SUMMARY_MAX_PARTS: usize = 16;
const SUMMARY_PART_OVERLAP_LINES: usize = 3;
const SUMMARY_MAP_MAX_TOKENS: u32 = 1024;

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

fn map_notes_prompt(lang: &str) -> String {
    format!(
        r###"You extract compact notes from one part of a longer transcript.

Language: Write the notes in "{lang}" (ISO 639-1). Quotes stay verbatim in the original language, each with its [HH:MM:SS] timestamp.

Rules:
1. No preamble, no closing remarks, no code fences.
2. Bullet points only: key statements, arguments, numbers, dates.
3. Include up to 5 notable verbatim quotes as: > "quote" [HH:MM:SS]
4. Stay faithful to this part only. Do not invent content from outside it."###
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

/// Reasoning models (o-series, and several behind OpenRouter) reject
/// `max_tokens` and a non-default `temperature`; they want
/// `max_completion_tokens` and nothing else. The wording differs per provider,
/// so match on the parameter names rather than on a model allow-list.
fn rejects_sampling_params(err: &str) -> bool {
    let l = err.to_ascii_lowercase();
    let names = ["max_tokens", "max_completion_tokens", "temperature"];
    let complaints = [
        "unsupported",
        "not supported",
        "unrecognized",
        "is not permitted",
        "does not support",
        "invalid",
        "use 'max_completion_tokens'",
        "use `max_completion_tokens`",
    ];
    names.iter().any(|n| l.contains(n)) && complaints.iter().any(|c| l.contains(c))
}

async fn call_llm(
    client: &Client<OpenAIConfig>,
    model: &str,
    temperature: f32,
    max_tokens: u32,
    system: &str,
    user: &str,
) -> Result<String, String> {
    match call_once(client, model, Some((temperature, max_tokens)), system, user).await {
        Err(e) if rejects_sampling_params(&e) => {
            // Second and last attempt: provider defaults for both.
            call_once(client, model, None, system, user).await
        }
        other => other,
    }
}

async fn call_once(
    client: &Client<OpenAIConfig>,
    model: &str,
    sampling: Option<(f32, u32)>,
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

    let mut builder = CreateChatCompletionRequestArgs::default();
    builder.model(model).messages(vec![
        ChatCompletionRequestMessage::System(sys),
        ChatCompletionRequestMessage::User(usr),
    ]);
    if let Some((temperature, max_tokens)) = sampling {
        builder.temperature(temperature).max_tokens(max_tokens);
    }
    let req = builder.build().map_err(|e| e.to_string())?;

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

fn looks_like_context_overflow(err: &str) -> bool {
    let l = err.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "context length",
        "context_length",
        "maximum context",
        "context window",
        "too many tokens",
        "token limit",
        "prompt is too long",
        "prompt too long",
        "maximum prompt",
        "max prompt",
        "exceeds the model's",
        "exceeds maximum",
        "reduce the length of the messages",
        "this model's maximum",
        "requested a too large",
        "string too long",
    ];
    NEEDLES.iter().any(|n| l.contains(n))
}

/// `max_part_chars` bounds one map call. `SUMMARY_MAX_PARTS` is a soft cap: a
/// transcript long enough that 16 parts would still exceed the budget gets more
/// parts instead, because the alternative is a request the model refuses.
fn split_transcript_parts_capped(transcript: &str, max_part_chars: usize) -> Vec<String> {
    let lines: Vec<&str> = transcript.split('\n').collect();
    if lines.is_empty() {
        return vec![transcript.to_string()];
    }
    let total = transcript.len().max(1);
    let budget = max_part_chars.max(1);
    let part_target = SUMMARY_MIN_PART_CHARS
        .max(total.div_ceil(SUMMARY_MAX_PARTS))
        .min(budget);
    let max_parts = SUMMARY_MAX_PARTS.max(total.div_ceil(part_target));
    let mut parts: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut cur_len = 0usize;

    for line in &lines {
        let add = line.len() + usize::from(!cur.is_empty());
        if !cur.is_empty() && cur_len + add > part_target && parts.len() < max_parts - 1 {
            parts.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        if !cur.is_empty() {
            cur_len += 1;
        }
        cur.push(*line);
        cur_len += line.len();
    }
    if !cur.is_empty() {
        parts.push(cur);
    }

    let mut out = Vec::with_capacity(parts.len());
    for i in 0..parts.len() {
        let mut chunk: Vec<&str> = Vec::new();
        if i > 0 {
            let prev = &parts[i - 1];
            let n = SUMMARY_PART_OVERLAP_LINES.min(prev.len());
            chunk.extend_from_slice(&prev[prev.len() - n..]);
        }
        chunk.extend_from_slice(&parts[i]);
        out.push(chunk.join("\n"));
    }
    if out.is_empty() {
        out.push(transcript.to_string());
    }
    out
}

fn whole_user_message(context: &str, transcript: &str) -> String {
    let mut user = String::new();
    if !context.trim().is_empty() {
        user.push_str(
            "Recording context (orientation only — summarize the transcript, not this block):\n",
        );
        user.push_str(context.trim());
        user.push_str("\n\n");
    }
    user.push_str("Transcript:\n\n");
    user.push_str(transcript);
    user
}

fn reduce_user_message(context: &str, notes: &[String]) -> String {
    let mut user = String::new();
    if !context.trim().is_empty() {
        user.push_str(
            "Recording context (orientation only — summarize the notes, not this block):\n",
        );
        user.push_str(context.trim());
        user.push_str("\n\n");
    }
    user.push_str(
        "These are sequential notes from parts of one recording. Produce the final summary from all of them.\n\n",
    );
    for (i, note) in notes.iter().enumerate() {
        user.push_str(&format!("## Part {}\n\n{}\n\n", i + 1, note.trim()));
    }
    user
}

async fn summarize_whole(
    client: &Client<OpenAIConfig>,
    cfg: &AppConfig,
    context: &str,
    transcript: &str,
) -> Result<String, String> {
    let lang = resolve_summary_language(&cfg.summary_language);
    let system = summary_system_prompt(&lang);
    let user = whole_user_message(context, transcript);
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

async fn summarize_chunked<F>(
    client: &Client<OpenAIConfig>,
    cfg: &AppConfig,
    context: &str,
    transcript: &str,
    on_part: F,
) -> Result<String, String>
where
    F: Fn(usize, usize),
{
    // Halve the part size and retry when the model reports a context overflow.
    // Without this a transcript whose 1/16 slice is still too large failed the
    // whole file, since only the single-call path had a fallback.
    let mut budget = SUMMARY_SINGLE_CALL_MAX_CHARS;
    loop {
        match summarize_chunked_at(client, cfg, context, transcript, &on_part, budget).await {
            Err(e) if looks_like_context_overflow(&e) && budget > SUMMARY_MIN_PART_CHARS => {
                budget = (budget / 2).max(SUMMARY_MIN_PART_CHARS);
            }
            other => return other,
        }
    }
}

async fn summarize_chunked_at<F>(
    client: &Client<OpenAIConfig>,
    cfg: &AppConfig,
    context: &str,
    transcript: &str,
    on_part: &F,
    max_part_chars: usize,
) -> Result<String, String>
where
    F: Fn(usize, usize),
{
    let parts = split_transcript_parts_capped(transcript, max_part_chars);
    let total = parts.len();
    let lang = resolve_summary_language(&cfg.summary_language);
    let map_system = map_notes_prompt(&lang);
    let mut notes = Vec::with_capacity(total);
    for (i, part) in parts.iter().enumerate() {
        on_part(i + 1, total);
        let mut user = String::new();
        if !context.trim().is_empty() {
            user.push_str(
                "Recording context (orientation only — extract notes from this part, not this block):\n",
            );
            user.push_str(context.trim());
            user.push_str("\n\n");
        }
        user.push_str(&format!("Transcript part {} of {total}:\n\n{part}", i + 1));
        let note = call_llm(
            client,
            &cfg.api_model,
            SUMMARY_TEMPERATURE,
            SUMMARY_MAP_MAX_TOKENS,
            &map_system,
            &user,
        )
        .await?;
        notes.push(note);
    }
    on_part(0, total);
    let system = summary_system_prompt(&lang);
    let user = reduce_user_message(context, &notes);
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

/// Output budget for improve/translate: proportional to the input, clamped.
fn rewrite_max_tokens(chars: usize) -> u32 {
    let estimate = (chars / 2).clamp(1, REWRITE_MAX_TOKENS as usize) as u32;
    estimate.clamp(REWRITE_MIN_TOKENS, REWRITE_MAX_TOKENS)
}

/// `context` is a short orientation block (title, podcast/episode info); may be empty.
///
/// `on_part(k, n)` reports map-reduce progress: `k` in 1..=n for each part,
/// `k == 0` for the combining step. Unused for a single-call summary.
pub async fn generate_summary<F>(
    client: &Client<OpenAIConfig>,
    cfg: &AppConfig,
    context: &str,
    transcript: &str,
    on_part: F,
) -> Result<String, String>
where
    F: Fn(usize, usize) + Send,
{
    if transcript.len() <= SUMMARY_SINGLE_CALL_MAX_CHARS {
        match summarize_whole(client, cfg, context, transcript).await {
            Ok(s) => Ok(s),
            Err(e) if looks_like_context_overflow(&e) => {
                summarize_chunked(client, cfg, context, transcript, on_part).await
            }
            Err(e) => Err(e),
        }
    } else {
        summarize_chunked(client, cfg, context, transcript, on_part).await
    }
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
/// Lines with no overlap inherit the previous speaker (then the next, for a
/// leading gap).
pub fn speakers_for_lines(
    lines: &[TranscriptLine],
    turns: &[(f32, f32, usize)],
) -> Vec<Option<usize>> {
    let mut assigned: Vec<Option<usize>> = lines
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
        .collect();
    inherit_speaker_gaps(&mut assigned);
    assigned
}

fn inherit_speaker_gaps(speakers: &mut [Option<usize>]) {
    let mut prev = None;
    for s in speakers.iter_mut() {
        if s.is_some() {
            prev = *s;
        } else if let Some(p) = prev {
            *s = Some(p);
        }
    }
    let first = speakers.iter().copied().find(Option::is_some).flatten();
    if let Some(first) = first {
        for s in speakers.iter_mut() {
            if s.is_some() {
                break;
            }
            *s = Some(first);
        }
    }
}

/// Replace a short outlier when both neighbours agree. Kept separate so a
/// bad calibration can drop it without touching overlap assignment.
pub fn smooth_speaker_outliers(lines: &[TranscriptLine], speakers: &mut [Option<usize>]) {
    const MAX_OUTLIER_S: f32 = 1.5;
    if speakers.len() < 3 || speakers.len() != lines.len() {
        return;
    }
    let orig = speakers.to_vec();
    for i in 1..speakers.len() - 1 {
        let dur = (lines[i].end - lines[i].start).max(0.0);
        if dur >= MAX_OUTLIER_S {
            continue;
        }
        if orig[i - 1] == orig[i + 1] && orig[i - 1].is_some() && orig[i] != orig[i - 1] {
            speakers[i] = orig[i - 1];
        }
    }
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
    if cfg.api_key.trim().is_empty() && !cfg.endpoint_is_local() {
        return Err("API key missing.".to_string());
    }
    let client = make_client(cfg);
    call_llm(
        &client,
        &cfg.api_model,
        SUMMARY_TEMPERATURE,
        rewrite_max_tokens(trimmed.len()),
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
    if cfg.api_key.trim().is_empty() && !cfg.endpoint_is_local() {
        return Err("API key missing.".to_string());
    }
    let client = make_client(cfg);
    let system = translate_system_prompt(target);
    call_llm(
        &client,
        &cfg.api_model,
        SUMMARY_TEMPERATURE,
        rewrite_max_tokens(trimmed.len()),
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
    // A model server on this machine authenticates nothing; requiring a key here
    // would make Verify impossible for the very providers that need none.
    if key.is_empty() && !cfg.endpoint_is_local() {
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
    if key.is_empty() && !cfg.endpoint_is_local() {
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
    let skip = is_non_chat_model;
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

/// Model ids that cannot serve a chat completion, filtered out of the picker.
fn is_non_chat_model(id: &str) -> bool {
    let l = id.to_ascii_lowercase();
    l.contains("embed")
        || l.contains("whisper")
        || l.contains("tts")
        || l.contains("moderation")
        || l.contains("rerank")
        || l.contains("image")
        || l.contains("video")
}

#[cfg(test)]
mod tests {
    use super::{
        fmt_ts, format_transcript, interval_overlap, looks_like_context_overflow, parse_model_ids,
        rejects_sampling_params, rewrite_max_tokens, smooth_speaker_outliers, speakers_for_lines,
        split_transcript_parts_capped, summary_system_prompt, TranscriptLine, REWRITE_MAX_TOKENS,
        REWRITE_MIN_TOKENS, SUMMARY_MAX_PARTS, SUMMARY_MIN_PART_CHARS,
        SUMMARY_SINGLE_CALL_MAX_CHARS,
    };

    /// The budget `summarize_chunked` starts from before any overflow retry.
    fn split_transcript_parts(transcript: &str) -> Vec<String> {
        split_transcript_parts_capped(transcript, SUMMARY_SINGLE_CALL_MAX_CHARS)
    }

    fn line(start: f32, end: f32, text: &str) -> TranscriptLine {
        TranscriptLine {
            start,
            end,
            text: text.into(),
        }
    }

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
    fn split_parts_stay_on_line_boundaries() {
        let mut lines = Vec::new();
        for i in 0..250 {
            lines.push(format!("[{i:02}:00:00] {}", "word ".repeat(120)));
        }
        let text = lines.join("\n");
        assert!(text.len() > SUMMARY_SINGLE_CALL_MAX_CHARS);
        let parts = split_transcript_parts(&text);
        assert!(parts.len() >= 2);
        assert!(parts.len() <= SUMMARY_MAX_PARTS);
        for part in &parts {
            assert!(part.starts_with('['), "{part:?}");
            assert!(part.len() >= SUMMARY_MIN_PART_CHARS / 4);
        }
        // Overlap: part 2 starts with the tail of part 1.
        let first_tail: Vec<&str> = parts[0].lines().rev().take(3).collect();
        let second_head: Vec<&str> = parts[1].lines().take(3).collect();
        assert_eq!(
            first_tail.into_iter().rev().collect::<Vec<_>>(),
            second_head
        );
    }

    #[test]
    fn split_caps_at_max_parts() {
        let line = format!("[00:00:00] {}", "x".repeat(50_000));
        let text = std::iter::repeat_n(line.as_str(), 16)
            .collect::<Vec<_>>()
            .join("\n");
        let parts = split_transcript_parts(&text);
        assert_eq!(parts.len(), SUMMARY_MAX_PARTS);
    }

    #[test]
    fn split_honours_a_smaller_part_budget() {
        let line = format!("[00:00:00] {}", "x".repeat(50_000));
        let text = std::iter::repeat_n(line.as_str(), 40)
            .collect::<Vec<_>>()
            .join("\n");
        // The default cut leaves 16 parts of ~125k chars — still too big for a
        // model that just refused 120k. A tighter budget must produce more,
        // smaller parts rather than silently keeping the oversized ones.
        let tight = split_transcript_parts_capped(&text, SUMMARY_MIN_PART_CHARS);
        let default = split_transcript_parts(&text);
        assert!(tight.len() > SUMMARY_MAX_PARTS, "{}", tight.len());
        assert!(default.len() < tight.len());
        for part in &tight {
            // One line already exceeds the budget, so allow a single-line
            // overshoot plus the 3 overlap lines carried from the part before.
            assert!(part.lines().count() <= 4, "{}", part.lines().count());
        }
    }

    #[test]
    fn rewrite_budget_scales_with_input() {
        assert_eq!(rewrite_max_tokens(0), REWRITE_MIN_TOKENS);
        assert_eq!(rewrite_max_tokens(100), REWRITE_MIN_TOKENS);
        assert_eq!(rewrite_max_tokens(20_000), 10_000);
        assert_eq!(rewrite_max_tokens(10_000_000), REWRITE_MAX_TOKENS);
    }

    #[test]
    fn sampling_param_rejections_are_recognised() {
        assert!(rejects_sampling_params(
            "Unsupported parameter: 'max_tokens' is not supported with this model. \
             Use 'max_completion_tokens' instead."
        ));
        assert!(rejects_sampling_params(
            "invalid_request_error: temperature does not support 0.3 with this model"
        ));
        assert!(!rejects_sampling_params("rate limit exceeded"));
        assert!(!rejects_sampling_params(
            "maximum context length is 8192 tokens"
        ));
    }

    #[test]
    fn context_overflow_needles() {
        assert!(looks_like_context_overflow(
            "This model's maximum context length is 8192 tokens"
        ));
        assert!(looks_like_context_overflow("prompt is too long"));
        assert!(!looks_like_context_overflow("invalid api key"));
        assert!(!looks_like_context_overflow("rate limit exceeded"));
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
    fn unlabeled_lines_inherit_neighbour() {
        let lines = [
            line(0.0, 1.0, "a"),
            line(1.0, 2.0, "b"),
            line(2.0, 3.0, "c"),
        ];
        let turns = [(1.0, 2.0, 2)];
        let assigned = speakers_for_lines(&lines, &turns);
        assert_eq!(assigned, vec![Some(2), Some(2), Some(2)]);
    }

    #[test]
    fn smooth_drops_short_outlier() {
        let lines = [
            line(0.0, 2.0, "a"),
            line(2.0, 2.8, "b"),
            line(2.8, 5.0, "c"),
        ];
        let mut speakers = vec![Some(1), Some(2), Some(1)];
        smooth_speaker_outliers(&lines, &mut speakers);
        assert_eq!(speakers, vec![Some(1), Some(1), Some(1)]);
        let mut long = vec![Some(1), Some(2), Some(1)];
        let long_lines = [
            line(0.0, 2.0, "a"),
            line(2.0, 4.0, "b"),
            line(4.0, 6.0, "c"),
        ];
        smooth_speaker_outliers(&long_lines, &mut long);
        assert_eq!(long, vec![Some(1), Some(2), Some(1)]);
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
