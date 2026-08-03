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

pub fn segments_to_raw_text(state: &whisper_rs::WhisperState) -> Result<String, String> {
    let n = state.full_n_segments();
    let mut lines = Vec::new();
    for i in 0..n {
        let Some(seg) = state.get_segment(i) else {
            continue;
        };
        let t0 = seg.start_timestamp() as f32 / 100.0;
        let text = seg.to_str_lossy().unwrap_or_default().trim().to_string();
        if text.is_empty() {
            continue;
        }
        lines.push(format!("{} {text}", fmt_ts(t0)));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{
        fmt_ts, summary_system_prompt, transcript_truncated_for_summary, SUMMARY_MAX_INPUT_CHARS,
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
}
