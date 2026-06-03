use serde_json::json;

/// A tool call resolved from either native API tool-use or a parsed locus tag.
pub(crate) struct NativeCall {
    pub id: String,
    pub action: ParsedAction,
}

pub(crate) struct ParsedAction {
    pub name: String,    // "read" | "write" | "search" | "list" | "query"
    pub content: String, // trimmed content between the tags
}

impl ParsedAction {
    /// First line = file path, remainder = file content (for write actions).
    pub fn write_parts(&self) -> (&str, &str) {
        if let Some(nl) = self.content.find('\n') {
            (self.content[..nl].trim(), &self.content[nl + 1..])
        } else {
            (self.content.trim(), "")
        }
    }

    pub fn to_tool_use_event(&self) -> serde_json::Value {
        match self.name.as_str() {
            "write"  => { let (p, _) = self.write_parts(); json!({"name": "write_file",     "input": {"path": p}}) }
            "read"   => json!({"name": "read_file",        "input": {"path":    self.content.trim()}}),
            "search" => json!({"name": "search_code",      "input": {"pattern": self.content.trim()}}),
            "list"   => json!({"name": "list_files",       "input": {"path":    self.content.trim()}}),
            "query"  => json!({"name": "query_database",   "input": {"query":   self.content.trim()}}),
            "schema" => json!({"name": "get_database_schema", "input": {}}),
            other    => json!({"name": other, "input": {}}),
        }
    }
}

// ─── Tag Interceptor ──────────────────────────────────────────────────────────
//
// Parses <locus:name>content</locus:name> tags from streaming text in real
// time. Text outside tags is returned immediately for display; tags are
// buffered silently and collected as ParsedActions when complete.

enum State {
    Normal,
    MaybeTag { buf: String },
    InBlock { name: String, content: String },
}

pub(crate) struct TagInterceptor {
    state: State,
    actions: Vec<ParsedAction>,
}

impl TagInterceptor {
    const PREFIX: &'static str = "<locus:";

    pub fn new() -> Self {
        TagInterceptor { state: State::Normal, actions: Vec::new() }
    }

    /// Feed a streaming chunk. Returns the text that should be shown to the user.
    pub fn process(&mut self, chunk: &str) -> String {
        let mut emit = String::new();
        for ch in chunk.chars() {
            match &mut self.state {
                State::Normal => {
                    if ch == '<' {
                        self.state = State::MaybeTag { buf: "<".to_string() };
                    } else {
                        emit.push(ch);
                    }
                }
                State::MaybeTag { buf } => {
                    buf.push(ch);
                    if buf.len() <= Self::PREFIX.len() {
                        if !Self::PREFIX.starts_with(buf.as_str()) {
                            let flushed = std::mem::take(buf);
                            emit.push_str(&flushed);
                            self.state = State::Normal;
                        }
                    } else if buf.starts_with(Self::PREFIX) {
                        if ch == '>' {
                            let name = buf[Self::PREFIX.len()..buf.len() - 1].trim().to_string();
                            self.state = State::InBlock { name, content: String::new() };
                        }
                    } else {
                        let flushed = std::mem::take(buf);
                        emit.push_str(&flushed);
                        self.state = State::Normal;
                    }
                }
                State::InBlock { name, content } => {
                    content.push(ch);
                    let close = format!("</locus:{}>", name);
                    if content.ends_with(&close) {
                        let raw = content[..content.len() - close.len()].to_string();
                        self.actions.push(ParsedAction {
                            name: name.clone(),
                            content: raw.trim_matches('\n').to_string(),
                        });
                        self.state = State::Normal;
                    }
                }
            }
        }
        emit
    }

    /// Call at end of stream — flushes any buffered non-tag text.
    pub fn flush(&mut self) -> String {
        let out = if let State::MaybeTag { buf } = &self.state { buf.clone() } else { String::new() };
        self.state = State::Normal;
        out
    }

    pub fn has_actions(&self) -> bool {
        !self.actions.is_empty()
    }

    pub fn take_actions(&mut self) -> Vec<ParsedAction> {
        std::mem::take(&mut self.actions)
    }
}
