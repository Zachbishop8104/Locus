# Locus

A desktop AI developer assistant.

Locus keeps your project context, source files, database schema, git repo, Jira board in one place and lets you have a focused conversation with Claude about it without leaving your workflow.

## Features

- **Project-aware chat** - attach a local path, git repo, Jira project, and SQL Server database to a conversation. Claude can read files, search code, and query the database directly.
- **Inline file editing** - Claude proposes file changes through a `write_file` tool. A Yes/No card appears in the chat for each change before anything is written to disk.
- **Permission modes** - switch between *Ask permission* (approve each edit) and *Auto edit* (apply changes immediately) from the message input bar.
- **Microsoft Teams** - browse teams, channels, and chats from your work account without switching apps.
- **Model selector** - switch between Claude Opus, Sonnet, and Haiku per conversation.
- **Local Claude** - optionally route requests through the Claude CLI instead of an API key.

## Tech Stack

| Layer | Tech |
|---|---|
| Desktop shell | Tauri 2 (Rust) |
| Frontend | Vue 3 + TypeScript + Vite |
| Styling | Tailwind CSS |
| AI | Anthropic Claude API |
| Database | SQL Server (via Tiberius) |

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org) 18+
- [Rust](https://rustup.rs) (stable)
- An [Anthropic API key](https://console.anthropic.com) - or install [Claude Code](https://claude.ai/code) and enable *Use local Claude* in settings

### Install & Run

```bash
npm install
npm run tauri dev
```

### Build

```bash
npm run tauri build
```

## Project Structure

```
src/                  Vue frontend
  components/chat/    Message bubbles, input bar, edit approval card
  views/              Chat, Projects, Teams, Settings
  stores/             Pinia stores (chat, projects, settings, teams)
  composables/        useClaudeStream - Tauri event bridge
src-tauri/src/
  claude.rs           Claude API integration, tool execution, streaming
  settings.rs         Config persistence
  teams.rs            Microsoft Teams OAuth + API
```
