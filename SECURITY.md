# Security Policy

## Supported Versions

Only the latest release receives security fixes.

| Version | Supported |
|---------|-----------|
| latest release | ✅ |
| older releases | ❌ |

## Reporting a Vulnerability

Please **do not** open a public issue for security problems. Instead, use
[GitHub private vulnerability reporting](https://github.com/fly2nbc-oss/VoxMD/security/advisories/new)
on this repository.

Include the VoxMD version, your OS, and steps to reproduce. You can expect an
initial response within 14 days.

## Scope Notes

- VoxMD runs entirely locally; the only outbound network traffic is: Whisper
  model downloads from Hugging Face, the OpenAI-compatible API endpoint you
  configure, and the podcast feed/episode URLs you add.
- Podcast episode audio is stored in the output folder you choose (alongside
  the Markdown). Optional post-export deletion removes that audio only — never
  the Markdown file.
- API keys are stored locally via the Tauri store plugin and are never sent
  anywhere except the configured API base URL.
