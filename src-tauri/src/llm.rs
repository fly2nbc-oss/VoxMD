use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
};
use async_openai::Client;

use crate::config::AppConfig;

const SYSTEM_PROMPT: &str = r#"Deine Aufgabe: Ein Rohtranskript (ohne Sprecherangaben) in ein Transkript mit Sprecherbezeichnungen umwandeln.

Anweisungen:
1. Erkenne Sprecherwechsel aus dem Kontext (Themenwechsel, Anrede, Frage/Antwort, typische Formulierungen).
2. Namen nur verwenden, wenn sie im Rohtext eindeutig als Personennamen vorkommen. Keine Nachnamen raten.
3. Im Zweifel: ausschließlich neutrale Bezeichnungen wie "Sprecher 1", "Sprecher 2" (fortlaufend).
4. Keine generischen Rollen wie "Interviewer", "Gast" – außer sie stehen wörtlich im Text.
5. Behalte jeden Zeitstempel [HH:MM:SS] exakt bei und setze ihn an den Anfang der Ausgabezeile.
6. Ausgabeformat: Genau eine Zeile pro Äußerung im Format:
   [HH:MM:SS] **Bezeichnung:** Der gesagte Text.
7. Wichtig: Gib das gesamte übergebene Rohtranskript vollständig wieder – kürzen ist nicht erlaubt."#;

const SYSTEM_PROMPT_CONT: &str = r#"Fortsetzung: Du bekommst den nächsten Abschnitt eines Rohtranskripts.

WICHTIG – Konsistenz der Sprecher:
- Verwende für bereits eingeführte Personen dieselbe Bezeichnung wie im vorherigen Abschnitt.
- Neue Person: nur echten Namen verwenden, wenn klar genannt; sonst nächste freie neutrale Bezeichnung.
- Behalte jeden Zeitstempel [HH:MM:SS] exakt bei.

Gib nur diesen Abschnitt vollständig im Format [HH:MM:SS] **Bezeichnung:** Text aus, ohne Einleitung."#;

const SYSTEM_PROMPT_SUMMARY: &str = r#"Erstelle aus dem folgenden Transkript eine strukturierte Zusammenfassung und extrahiere Zitate.

Anweisungen:
    Fokus: Extrahiere ausschließlich die Kernaussagen. Ignoriere Smalltalk und Werbung.
    Struktur: Verwende genau diese Markdown-Gliederung:
        ## Metadaten
        Datum der Folge/Veröffentlichung, Episodennummer. Wenn nichts genannt: "Keine Metadaten genannt."
        ## Zusammenfassung in einem Satz
        Worum geht es im Kern? Ein einziger prägnanter Satz.
        ## Die wichtigsten Thesen & Erkenntnisse
        Bullet Points mit den Kernaussagen.
        ## Daten & Fakten
        Alle signifikanten Zahlen, Statistiken oder Termine.
        ## Wichtigste Zitate
        Wähle 5 bis 10 Zitate, die außergewöhnliche Daten, konkrete Fakten oder besonders prägnante Aussagen enthalten. Wiedergebe jedes Zitat wörtlich. Pro Zitat eine Zeile im Format: **Bezeichnung:** "wörtliches Zitat" – die Bezeichnung exakt aus dem Transkript übernehmen (keine Namen erfinden; bei neutralen Labels diese verwenden).
    Stil: Sachlich, prägnant, informativ. Keine Füllwörter.
    Limit: Zusammenfassung maximal ca. 800 Wörter; Zitate wörtlich aus dem Text."#;

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
            format!("Rohtranskript:\n\n{chunk}")
        } else {
            format!("Nächster Abschnitt:\n\n{chunk}")
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
        s.push_str("\n\n[... Transkript gekürzt für Zusammenfassung ...]");
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
        &format!("Transkript:\n\n{text_for_summary}"),
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
