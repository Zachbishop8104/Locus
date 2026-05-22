# Graph Report - .  (2026-05-22)

## Corpus Check
- Corpus is ~16,988 words - fits in a single context window. You may not need a graph.

## Summary
- 260 nodes · 297 edges · 36 communities (26 shown, 10 thin omitted)
- Extraction: 95% EXTRACTED · 5% INFERRED · 0% AMBIGUOUS · INFERRED: 14 edges (avg confidence: 0.84)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Teams & OAuth Integration|Teams & OAuth Integration]]
- [[_COMMUNITY_Claude API Integration|Claude API Integration]]
- [[_COMMUNITY_Tauri App Configuration|Tauri App Configuration]]
- [[_COMMUNITY_Frontend Dev Dependencies|Frontend Dev Dependencies]]
- [[_COMMUNITY_Node TypeScript Config|Node TypeScript Config]]
- [[_COMMUNITY_Teams Chat View|Teams Chat View]]
- [[_COMMUNITY_App TypeScript Config|App TypeScript Config]]
- [[_COMMUNITY_Project Entry & README|Project Entry & README]]
- [[_COMMUNITY_TypeScript Data Types|TypeScript Data Types]]
- [[_COMMUNITY_Frontend Runtime Dependencies|Frontend Runtime Dependencies]]
- [[_COMMUNITY_Teams Data Store|Teams Data Store]]
- [[_COMMUNITY_Settings & API Key Management|Settings & API Key Management]]
- [[_COMMUNITY_Brand & Visual Identity|Brand & Visual Identity]]
- [[_COMMUNITY_Social Icons Sprite|Social Icons Sprite]]
- [[_COMMUNITY_Tauri Default Capabilities|Tauri Default Capabilities]]
- [[_COMMUNITY_Sidebar Conversation UI|Sidebar Conversation UI]]
- [[_COMMUNITY_Teams Avatar Component|Teams Avatar Component]]
- [[_COMMUNITY_Chat View|Chat View]]
- [[_COMMUNITY_Claude Code Hooks Config|Claude Code Hooks Config]]
- [[_COMMUNITY_TypeScript Config Root|TypeScript Config Root]]
- [[_COMMUNITY_Claude Stream Composable|Claude Stream Composable]]
- [[_COMMUNITY_Projects Store|Projects Store]]
- [[_COMMUNITY_Settings Store|Settings Store]]
- [[_COMMUNITY_Chat Store|Chat Store]]
- [[_COMMUNITY_VS Code Config|VS Code Config]]

## God Nodes (most connected - your core abstractions)
1. `compilerOptions` - 16 edges
2. `get_valid_token()` - 14 edges
3. `compilerOptions` - 10 edges
4. `read_config()` - 10 edges
5. `start_teams_auth()` - 8 edges
6. `execute_tool()` - 7 edges
7. `write_config()` - 7 edges
8. `query_sqlserver()` - 6 edges
9. `save_tokens()` - 6 edges
10. `Icons Sprite Sheet SVG (Social & UI Icons)` - 6 edges

## Surprising Connections (you probably didn't know these)
- `Locus Logo SVG (Light Theme, Root)` --semantically_similar_to--> `Locus Logo SVG (Light Theme, Assets)`  [INFERRED] [semantically similar]
  locus-logo.svg → src/assets/locus-logo.svg
- `Locus Logo SVG (Light Theme, Root)` --semantically_similar_to--> `Locus Logo SVG (Dark Theme, Light Colors)`  [INFERRED] [semantically similar]
  locus-logo.svg → src/assets/locus-logo-dark.svg
- `Favicon SVG (Claude/Bolt-style Lightning Icon)` --conceptually_related_to--> `Locus Brand Identity (Compass Rose + Wordmark)`  [INFERRED]
  public/favicon.svg → locus-logo.svg
- `Hero Image (Isometric Layered Boxes, Purple Gradient)` --conceptually_related_to--> `Locus Brand Identity (Compass Rose + Wordmark)`  [INFERRED]
  src/assets/hero.png → locus-logo.svg
- `Vue 3 Framework` --conceptually_related_to--> `index.html Entry Point`  [INFERRED]
  README.md → index.html

## Communities (36 total, 10 thin omitted)

### Community 0 - "Teams & OAuth Integration"
Cohesion: 0.12
Nodes (27): ChannelInfo, ChatInfo, code_challenge(), code_verifier(), fetch_teams_image(), get_channel_messages(), get_chat_messages(), get_chats() (+19 more)

### Community 1 - "Claude API Integration"
Cohesion: 0.15
Nodes (24): auto_recover_udt(), BlockBuilder, build_tools(), cell_to_string(), collect_files(), do_stream(), execute_tool(), extract_primary_table() (+16 more)

### Community 2 - "Tauri App Configuration"
Cohesion: 0.10
Nodes (19): debugApplicationIdSuffix, app, security, windows, build, beforeBuildCommand, beforeDevCommand, devUrl (+11 more)

### Community 3 - "Frontend Dev Dependencies"
Cohesion: 0.11
Nodes (18): devDependencies, tailwindcss, @tailwindcss/typography, @tailwindcss/vite, @types/node, typescript, vite, @vitejs/plugin-vue (+10 more)

### Community 4 - "Node TypeScript Config"
Cohesion: 0.11
Nodes (17): compilerOptions, allowImportingTsExtensions, erasableSyntaxOnly, lib, module, moduleDetection, moduleResolution, noEmit (+9 more)

### Community 5 - "Teams Chat View"
Cohesion: 0.14
Nodes (8): bodyHtml(), CARD_TYPES, chatAvatarName(), chatLabel(), sanitizeTeamsHtml(), selectedChannel, selectedChat, selectedTeam

### Community 6 - "App TypeScript Config"
Cohesion: 0.14
Nodes (13): compilerOptions, baseUrl, erasableSyntaxOnly, ignoreDeprecations, noFallthroughCasesInSwitch, noUnusedLocals, noUnusedParameters, paths (+5 more)

### Community 7 - "Project Entry & README"
Cohesion: 0.20
Nodes (9): App Root Mount Point (#app), index.html Entry Point, README - Vue 3 + TypeScript + Vite Project, Vue 3 Script Setup SFCs, TypeScript, Vite Build Tool, Vue 3 Framework, app (+1 more)

### Community 8 - "TypeScript Data Types"
Cohesion: 0.20
Nodes (9): Conversation, DB_TYPES, DbType, Message, ModelId, ModelOption, MODELS, Project (+1 more)

### Community 9 - "Frontend Runtime Dependencies"
Cohesion: 0.22
Nodes (9): dependencies, @lucide/vue, marked, pinia, pinia-plugin-persistedstate, @tauri-apps/api, @types/marked, vue (+1 more)

### Community 10 - "Teams Data Store"
Cohesion: 0.22
Nodes (7): ChannelInfo, ChatInfo, TeamInfo, TeamsAttachment, TeamsMessage, TeamsStatus, useTeamsStore

### Community 11 - "Settings & API Key Management"
Cohesion: 0.47
Nodes (8): config_path(), get_api_key(), get_use_local_claude(), read_config(), set_api_key(), set_use_local_claude(), write_config(), disconnect_teams()

### Community 12 - "Brand & Visual Identity"
Cohesion: 0.52
Nodes (7): Compass Rose Design Element, Locus Brand Identity (Compass Rose + Wordmark), Favicon SVG (Claude/Bolt-style Lightning Icon), Hero Image (Isometric Layered Boxes, Purple Gradient), Locus Logo SVG (Dark Theme, Light Colors), Locus Logo SVG (Light Theme, Assets), Locus Logo SVG (Light Theme, Root)

### Community 13 - "Social Icons Sprite"
Cohesion: 0.29
Nodes (7): Bluesky Social Icon (Symbol in icons.svg), Discord Icon (Symbol in icons.svg), Documentation Icon (Symbol in icons.svg), GitHub Icon (Symbol in icons.svg), Social/User Icon (Symbol in icons.svg), X (Twitter) Icon (Symbol in icons.svg), Icons Sprite Sheet SVG (Social & UI Icons)

### Community 14 - "Tauri Default Capabilities"
Cohesion: 0.33
Nodes (5): description, identifier, permissions, $schema, windows

## Knowledge Gaps
- **120 isolated node(s):** `name`, `private`, `version`, `type`, `dev` (+115 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **10 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `dependencies` connect `Frontend Runtime Dependencies` to `Frontend Dev Dependencies`?**
  _High betweenness centrality (0.005) - this node is a cross-community bridge._
- **Why does `read_config()` connect `Settings & API Key Management` to `Teams & OAuth Integration`?**
  _High betweenness centrality (0.004) - this node is a cross-community bridge._
- **Are the 4 inferred relationships involving `read_config()` (e.g. with `get_teams_config()` and `save_tokens()`) actually correct?**
  _`read_config()` has 4 INFERRED edges - model-reasoned connections that need verification._
- **What connects `name`, `private`, `version` to the rest of the system?**
  _122 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Teams & OAuth Integration` be split into smaller, more focused modules?**
  _Cohesion score 0.12315270935960591 - nodes in this community are weakly interconnected._
- **Should `Tauri App Configuration` be split into smaller, more focused modules?**
  _Cohesion score 0.1 - nodes in this community are weakly interconnected._
- **Should `Frontend Dev Dependencies` be split into smaller, more focused modules?**
  _Cohesion score 0.10526315789473684 - nodes in this community are weakly interconnected._