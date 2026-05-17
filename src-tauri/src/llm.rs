use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
};
use async_openai::Client;

use crate::config::AppConfig;

const SYSTEM_PROMPT: &str = r#"Your task: Convert a raw transcript (without speaker labels) into a transcript with speaker designations.

Instructions:
1. Identify speaker changes from context (topic shifts, forms of address, question/answer patterns, typical phrasing).
2. Use names only if they appear unambiguously as personal names in the raw text. Do not guess last names.
3. When in doubt: use only neutral labels such as "Speaker 1", "Speaker 2" (sequential).
4. No generic roles like "Interviewer", "Guest" — unless they appear verbatim in the text.
5. Preserve every timestamp [HH:MM:SS] exactly and place it at the beginning of the output line.
6. Output format: Exactly one line per utterance in the format:
   [HH:MM:SS] **Label:** The spoken text.
7. Important: Reproduce the entire raw transcript passed to you completely — truncation is not allowed."#;

const SYSTEM_PROMPT_CONT: &str = r#"Continuation: You are receiving the next section of a raw transcript.

IMPORTANT — Speaker consistency:
- Use the same label for people already introduced in the previous section.
- New person: only use a real name if clearly stated; otherwise use the next available neutral label.
- Preserve every timestamp [HH:MM:SS] exactly.

Output only this section completely in the format [HH:MM:SS] **Label:** Text, without any introduction."#;

const SYSTEM_PROMPT_SUMMARY: &str = r#"Create a structured summary from the following transcript and extract quotes.

Instructions:
    Focus: Extract only the key statements. Ignore small talk and advertisements.
    Structure: Use exactly this Markdown outline:
        ## Metadata
        Date of episode/publication, episode number. If nothing mentioned: "No metadata mentioned."
        ## Summary in One Sentence
        What is the core topic? One single concise sentence.
        ## Key Arguments & Insights
        Bullet points with the core statements.
        ## Data & Facts
        All significant numbers, statistics, or dates.
        ## Most Important Quotes
        Select 5 to 10 quotes containing exceptional data, concrete facts, or particularly concise statements. Reproduce each quote verbatim. One quote per line in the format: **Label:** "verbatim quote" — use the label exactly as it appears in the transcript (do not invent names; use neutral labels where applicable).
    Style: Factual, concise, informative. No filler words.
    Limit: Summary maximum approx. 800 words; quotes verbatim from the text."#;

fn split_chunks(text: &str, max_chars: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        let mut end = (start + max_chars).min(text.len());
        if end < text.len() {
            let segment = &text[start..end];
            if let Some(pos) = segment.rfind(". ") {
                if pos > max_chars / 2 {
                    end = start + pos + 1;
                }
            }
        }
        let chunk = text[start..end].trim();
        if !chunk.is_empty() {
            chunks.push(chunk.to_string());
        }
        start = end;
    }
    chunks
}

pub fn make_client(cfg: &AppConfig) -> Client<OpenAIConfig> {
    let oc = OpenAIConfig::new()
        .with_api_base(cfg.api_base_url.trim_end_matches('/').to_string())
        .with_api_key(cfg.api_key.clone());
    Client::with_config(oc)
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

    let resp = client
        .chat()
        .create(req)
        .await
        .map_err(|e| e.to_string())?;

    Ok(resp
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default()
        .trim()
        .to_string())
}

pub async fn transcript_with_speakers_with_progress(
    client: &Client<OpenAIConfig>,
    cfg: &AppConfig,
    raw: &str,
    on_chunk: impl Fn(usize, usize),
) -> Result<String, String> {
    let chunks = split_chunks(raw, cfg.transcript_chunk_chars);
    if chunks.is_empty() {
        return Ok(raw.to_string());
    }
    let n = chunks.len();
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        on_chunk(i + 1, n);
        let sys = if i == 0 {
            SYSTEM_PROMPT
        } else {
            SYSTEM_PROMPT_CONT
        };
        let user = if i == 0 {
            format!("Raw transcript:\n\n{chunk}")
        } else {
            format!("Next section:\n\n{chunk}")
        };
        let part = call_llm(
            client,
            &cfg.api_model,
            cfg.temperature,
            cfg.max_tokens,
            sys,
            &user,
        )
        .await?;
        if !part.is_empty() {
            out.push(part);
        }
    }
    Ok(out.join("\n\n"))
}

pub async fn generate_summary(
    client: &Client<OpenAIConfig>,
    cfg: &AppConfig,
    transcript: &str,
) -> Result<String, String> {
    let max_in = 50_000usize;
    let text_for_summary = if transcript.len() > max_in {
        let mut s = transcript.chars().take(max_in).collect::<String>();
        s.push_str("\n\n[... transcript truncated for summary ...]");
        s
    } else {
        transcript.to_string()
    };
    call_llm(
        client,
        &cfg.api_model,
        cfg.temperature,
        cfg.max_tokens,
        SYSTEM_PROMPT_SUMMARY,
        &format!("Transcript:\n\n{text_for_summary}"),
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
        let Some(seg) = state.get_segment(i as i32) else {
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
