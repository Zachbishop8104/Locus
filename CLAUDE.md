## Rust Module Convention

Modules follow the Rust 2018 style: a module named `foo` is declared in `foo.rs` alongside a `foo/` directory for its submodules — never `foo/mod.rs`. Example: `agent.rs` is the root of the agent module; `agent/interceptor.rs`, `agent/stream.rs`, and `agent/tools.rs` are its submodules declared inside `agent.rs` with `mod interceptor;` etc.

## AI Agent Architecture

The AI layer (`src-tauri/src/agent.rs`) is model-agnostic. All providers — Claude API, local Claude CLI, and any OpenAI-compatible model (Ollama, LM Studio, etc.) — run through the same agentic loop.

**Adding a new capability is one thing:** add an instruction to `tool_system_prompt()` and a case in `execute_action()`. No Anthropic schema, no OpenAI schema, no frontend changes.

**How it works:** The model expresses intent by writing `<locus:name>content</locus:name>` tags in its response text. The `TagInterceptor` parses these out of the stream in real time, hides them from the user, executes the action, and injects the result as a user message before the next turn.

**Models that can follow instructions work.** Claude, Qwen3, Llama3, Mistral — anything that can be told "write `<locus:read>path</locus:read>` when you want to read a file" will work. The fallback when a model ignores the tags is graceful — `actions.is_empty()` returns true and the loop ends cleanly.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
