# Install

Install the CLI from crates.io:

```bash
cargo install pi-coding-agent
```

The crate name is `pi-coding-agent`; the installed binary is `pi-rs`.

## Environment variables

`pi-rs` reads its API key from the environment based on which model you target:

| Variable | Provider |
|----------|----------|
| `ANTHROPIC_API_KEY` | Anthropic Messages (Claude) |
| `OPENAI_API_KEY` | OpenAI Chat Completions, and any OpenAI-compatible endpoint (OpenRouter, Groq, Together, Cerebras, DeepSeek, Fireworks, xAI, ...) |
| `GOOGLE_API_KEY` or `GEMINI_API_KEY` | Google Generative AI (Gemini) |

```bash
export ANTHROPIC_API_KEY=sk-ant-...
pi-rs -p "Say hi"
```

You can also pick the active model explicitly with `PI_MODEL`:

```bash
PI_MODEL=claude-opus-4-7   pi-rs -p "..."   # Anthropic
PI_MODEL=gpt-4o            pi-rs -p "..."   # OpenAI
PI_MODEL=gemini-2.0-flash  pi-rs -p "..."   # Google
```
