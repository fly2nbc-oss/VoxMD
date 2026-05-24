use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
};
use async_openai::Client;

use crate::config::{resolve_summary_language, AppConfig};

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
- Use the EXACT same labels as in the "Previously labeled transcript lines" block when given — do not rename speakers (e.g. do not switch "Paul" to "Speaker B").
- Use the same label for people already introduced in the previous section.
- New person: only use a real name if clearly stated; otherwise use the next available neutral label (Speaker 1, Speaker 2, …).
- Preserve every timestamp [HH:MM:SS] exactly.

Output only this section completely in the format [HH:MM:SS] **Label:** Text, without any introduction."#;

const SYSTEM_PROMPT_REPAIR: &str = r#"You repair transcript formatting only.

The previous model output violated the required line format or timestamp order.

Output ONLY the corrected transcript for THIS raw section — no preamble, no headings, no commentary.

Hard rules:
1. Every non-empty line MUST match exactly: [HH:MM:SS] **Label:** spoken text (Label may contain spaces; use **Label:** with double asterisks).
2. Preserve every timestamp from the raw section exactly — same order as in the raw section (monotonic times).
3. Re-use speaker labels from "Previously labeled transcript lines" when provided.
4. One utterance per line (no paragraph merges without splitting lines)."#;

fn summary_system_prompt(lang: &str) -> String {
    format!(
        r#"Create a structured summary from the following transcript and extract quotes.

Language: Write the ENTIRE output in {lang} (ISO 639-1). All Markdown section headings, bullet points, metadata fields, and explanatory text must be in {lang}.

Instructions:
    Focus: Extract only the key statements. Ignore small talk and advertisements.
    Structure: Use exactly this Markdown outline with section headings in {lang} (same meaning as these sections):
        ## Metadata — date of episode/publication, episode number. If nothing mentioned, state that no metadata was mentioned (in {lang}).
        ## Summary in One Sentence — core topic in one concise sentence.
        ## Key Arguments & Insights — bullet points with the core statements.
        ## Data & Facts — all significant numbers, statistics, or dates.
        ## Most Important Quotes — select 5 to 10 quotes with exceptional data, concrete facts, or particularly concise statements. Reproduce each quote verbatim. One quote per line: **Label:** "verbatim quote" — use the label exactly as it appears in the transcript (do not invent names; use neutral labels where applicable).
    Style: Factual, concise, informative. No filler words.
    Limit: Summary maximum approx. 800 words; quotes verbatim from the text."#
    )
}

/// Prefer splitting at Whisper lines (`[HH:MM:SS] …`), never mid-timestamp when avoidable.
fn split_chunks(text: &str, max_chars: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }
    if max_chars == 0 {
        return vec![text.to_string()];
    }
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in lines {
        let line = line.trim_end();
        let extra = if current.is_empty() {
            line.len()
        } else {
            line.len().saturating_add(1)
        };

        if current.len() + extra > max_chars && !current.is_empty() {
            chunks.push(current.trim().to_string());
            current.clear();
        }

        // Single line exceeds budget: split inside line (sentence boundary preferred).
        if line.len() > max_chars {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
                current.clear();
            }
            chunks.extend(split_long_line(line, max_chars));
            continue;
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }

    if !current.is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

fn split_long_line(line: &str, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        if rest.len() <= max_chars {
            out.push(rest.trim().to_string());
            break;
        }
        let mut cut = byte_boundary_before(rest, max_chars);
        let window = &rest[..cut];
        if let Some(rel) = window.rfind(". ") {
            let candidate = rel.saturating_add(2);
            if candidate >= max_chars / 4 && candidate <= cut {
                cut = candidate;
            }
        }
        out.push(rest[..cut].trim().to_string());
        rest = rest[cut..].trim_start();
    }
    out
}

fn byte_boundary_before(s: &str, mut max_bytes: usize) -> usize {
    max_bytes = max_bytes.min(s.len());
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end.max(1).min(s.len())
}

fn parse_ts_seconds(ts_bracketed: &str) -> Option<i32> {
    let inner = ts_bracketed.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = inner.split(':');
    let h: i32 = parts.next()?.parse().ok()?;
    let m: i32 = parts.next()?.parse().ok()?;
    let s: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(h * 3600 + m * 60 + s)
}

/// Validates labeled transcript chunk from the LLM (English prompts → ASCII timestamps).
fn validate_labeled_chunk(output: &str) -> Result<(), String> {
    let mut last_ts = -1i32;
    let mut nonempty_lines = 0usize;

    for raw_line in output.lines() {
        let line = raw_line.trim().trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        nonempty_lines += 1;

        let ts_end = line
            .find(']')
            .ok_or_else(|| format!("Missing closing bracket on line: {}", truncate(line, 120)))?;
        let ts_part = &line[..=ts_end];
        let ts_secs =
            parse_ts_seconds(ts_part).ok_or_else(|| format!("Bad timestamp: {ts_part}"))?;

        if ts_secs < last_ts {
            return Err(format!(
                "Timestamps went backwards within chunk: {} after {}",
                ts_part,
                last_ts
            ));
        }
        last_ts = ts_secs;

        let after_ts = line.get(ts_end + 1..).unwrap_or("").trim_start();
        let rest = after_ts
            .strip_prefix("**")
            .ok_or_else(|| format!("Expected **Label:** after timestamp on: {}", truncate(line, 120)))?;

        let close = rest
            .find(":**")
            .ok_or_else(|| format!("Expected **Label:** closing on: {}", truncate(line, 120)))?;

        let label = &rest[..close];
        if label.trim().is_empty() {
            return Err("Empty speaker label".to_string());
        }

        let text_after = rest.get(close + 3..).unwrap_or("").trim_start();
        if text_after.is_empty() {
            return Err(format!(
                "Missing spoken text after label on: {}",
                truncate(line, 120)
            ));
        }
    }

    if nonempty_lines == 0 {
        return Err("Chunk output has no transcript lines".to_string());
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end.max(1)])
}

/// Last non-empty labeled lines from prior chunk output (speaker continuity).
fn tail_labeled_context(prev_output: &str, max_lines: usize, max_chars: usize) -> String {
    let lines: Vec<&str> = prev_output
        .lines()
        .map(|l| l.trim().trim_end_matches('\r'))
        .filter(|l| !l.is_empty())
        .collect();

    let start = lines.len().saturating_sub(max_lines);
    let tail_lines = &lines[start..];
    let mut s = tail_lines.join("\n");
    while s.len() > max_chars && !s.is_empty() {
        // Drop oldest lines until under budget.
        if let Some(idx) = s.find('\n') {
            s = s[idx + 1..].to_string();
        } else {
            s.clear();
            break;
        }
    }
    s
}

async fn transcribe_one_chunk(
    client: &Client<OpenAIConfig>,
    cfg: &AppConfig,
    chunk_idx: usize,
    chunk: &str,
    speaker_tail: &str,
    temperature: f32,
    repair_temperature: f32,
) -> Result<String, String> {
    let (system_base, user_first) = if chunk_idx == 0 {
        (SYSTEM_PROMPT, format!("Raw transcript:\n\n{chunk}"))
    } else {
        let user = if speaker_tail.is_empty() {
            format!("Next section:\n\n{chunk}")
        } else {
            format!(
                "Previously labeled transcript lines (keep speaker labels consistent — reuse names exactly):\n\n{speaker_tail}\n\n---\n\nNext section:\n\n{chunk}"
            )
        };
        (SYSTEM_PROMPT_CONT, user)
    };

    let mut part = call_llm(
        client,
        &cfg.api_model,
        temperature,
        cfg.max_tokens,
        system_base,
        &user_first,
    )
    .await?;

    if validate_labeled_chunk(&part).is_ok() {
        return Ok(part);
    }

    let mut repair_user = String::new();
    if !speaker_tail.is_empty() {
        repair_user.push_str(
            "### Previously labeled lines (reuse these speaker names)\n\n",
        );
        repair_user.push_str(speaker_tail);
        repair_user.push_str("\n\n");
    }
    repair_user.push_str("### Incorrect previous output\n\n");
    repair_user.push_str(&part);
    repair_user.push_str("\n\n### Raw section (correct every line)\n\n");
    repair_user.push_str(chunk);

    part = call_llm(
        client,
        &cfg.api_model,
        repair_temperature,
        cfg.max_tokens,
        SYSTEM_PROMPT_REPAIR,
        &repair_user,
    )
    .await?;

    validate_labeled_chunk(&part).map_err(|e| {
        format!(
            "LLM transcript chunk {} failed format validation after retry: {}",
            chunk_idx + 1,
            e
        )
    })?;

    Ok(part)
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
    const CONTEXT_TAIL_LINES: usize = 18;
    const CONTEXT_MAX_CHARS: usize = 4500;

    let chunks = split_chunks(raw, cfg.transcript_chunk_chars);
    if chunks.is_empty() {
        return Ok(raw.to_string());
    }
    let n = chunks.len();
    let repair_temp = (cfg.temperature * 0.5).clamp(0.0, 0.25);

    let mut out = Vec::new();
    let mut speaker_tail = String::new();

    for (i, chunk) in chunks.iter().enumerate() {
        on_chunk(i + 1, n);
        let part = transcribe_one_chunk(
            client,
            cfg,
            i,
            chunk,
            &speaker_tail,
            cfg.temperature,
            repair_temp,
        )
        .await?;
        speaker_tail = tail_labeled_context(&part, CONTEXT_TAIL_LINES, CONTEXT_MAX_CHARS);
        out.push(part);
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
    let lang = resolve_summary_language(&cfg.summary_language);
    let system = summary_system_prompt(&lang);
    call_llm(
        client,
        &cfg.api_model,
        cfg.temperature,
        cfg.max_tokens,
        &system,
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

#[cfg(test)]
mod tests {
    use super::validate_labeled_chunk;

    #[test]
    fn validates_standard_labeled_line() {
        let output = "[00:00:00] **Paul:** Es ist Freitag in der 15. Woche 2026 und hier ist nacktes Geld.";

        assert!(validate_labeled_chunk(output).is_ok());
    }

    #[test]
    fn rejects_missing_label_colon() {
        let output = "[00:00:00] **Paul** Es ist Freitag.";

        assert!(validate_labeled_chunk(output).is_err());
    }
}
