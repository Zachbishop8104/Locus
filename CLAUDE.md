## Rust Module Convention

Modules follow the Rust 2018 style: a module named `foo` is declared in `foo.rs` alongside a `foo/` directory for its submodules — never `foo/mod.rs`. Example: `agent.rs` is the root of the agent module; `agent/interceptor.rs`, `agent/stream.rs`, and `agent/tools.rs` are its submodules declared inside `agent.rs` with `mod interceptor;` etc.

## AI Agent Architecture

The AI layer (`src-tauri/src/agent.rs`) is model-agnostic. All providers — Claude API, local Claude CLI, and any OpenAI-compatible model (Ollama, LM Studio, etc.) — run through the same agentic loop. The frontend picks the provider per message (`claude_api` | `claude_cli` | `local`) along with the model, effort, and permission mode (`ask` | `edits` | `plan`).

**Two ways the model calls tools:**
- **Claude API and local models** use native tool calling. Schemas live in `anthropic_tools_schema()` (the OpenAI schema is derived from it) and map back to actions in `native_tool_to_action()`.
- **Claude CLI** uses tags: the model writes `<locus:name>content</locus:name>` in its response, and `TagInterceptor` parses these out of the stream in real time, hides them from the user, and the loop executes them and injects the result before the next turn. Instructions live in `tool_system_prompt()`.

**Adding a new capability:** add a case in `execute_action()`, a schema in `anthropic_tools_schema()` plus a mapping in `native_tool_to_action()`, and a tag instruction in `tool_system_prompt()`. No frontend changes are needed unless it needs new UI.

**Models that can follow instructions work.** Claude, Qwen3, Llama3, Mistral. The fallback when a model ignores the tools is graceful — no actions means the loop ends cleanly.
