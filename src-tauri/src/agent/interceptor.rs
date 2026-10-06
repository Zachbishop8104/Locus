use serde_json::json;

/// A tool call resolved from either native API tool-use or a parsed locus tag.
pub(crate) struct NativeCall {
    pub id: String,
    pub action: ParsedAction,
}

pub(crate) struct ParsedAction {
    pub name: String,    // "read" | "write" | "edit" | "search" | "list" | "query" | "schema"
    pub content: String, // trimmed content between the tags
    /// Structured arguments from a native tool call; Null for tag-based calls.
    pub args: serde_json::Value,
}

/// One exact-match replacement within a file.
pub(crate) struct EditHunk {
    pub old: String,
    pub new: String,
}

pub(crate) struct EditSpec {
    pub path: String,
    pub hunks: Vec<EditHunk>,
    pub replace_all: bool,
}

impl ParsedAction {
    pub fn new(name: &str, content: impl Into<String>) -> Self {
        ParsedAction { name: name.to_string(), content: content.into(), args: serde_json::Value::Null }
    }

    /// Edit arguments, from native tool-call args or from a tag of the form:
    ///
    /// ```text
    /// path/to/file
    /// <<<<<<< SEARCH
    /// exact existing text
    /// =======
    /// replacement text
    /// >>>>>>> REPLACE
    /// ```
    /// A tag may contain several SEARCH/REPLACE blocks.
    pub fn edit_parts(&self) -> Result<EditSpec, String> {
        if let (Some(path), Some(old), Some(new)) = (
            self.args["path"].as_str(), self.args["old_string"].as_str(), self.args["new_string"].as_str(),
        ) {
            return Ok(EditSpec {
                path: path.to_string(),
                hunks: vec![EditHunk { old: old.to_string(), new: new.to_string() }],
                replace_all: self.args["replace_all"].as_bool().unwrap_or(false),
            });
        }

        let (path, body) = self.write_parts();
        enum S { Outside, Search, Replace }
        let mut state = S::Outside;
        let (mut old, mut new) = (Vec::new(), Vec::new());
        let mut hunks = Vec::new();
        for line in body.split('\n') {
            let marker = line.trim_end_matches('\r');
            match state {
                S::Outside if marker.trim() == "<<<<<<< SEARCH" => state = S::Search,
                S::Outside => {}
                S::Search if marker == "=======" => state = S::Replace,
                S::Search => old.push(line),
                S::Replace if marker.trim() == ">>>>>>> REPLACE" => {
                    hunks.push(EditHunk { old: old.join("\n"), new: new.join("\n") });
                    old.clear();
                    new.clear();
                    state = S::Outside;
                }
                S::Replace => new.push(line),
            }
        }
        if path.is_empty() || hunks.is_empty() {
            return Err("Error: malformed edit. Put the file path on the first line, then one or more \
                <<<<<<< SEARCH / ======= / >>>>>>> REPLACE blocks.".to_string());
        }
        Ok(EditSpec { path: path.to_string(), hunks, replace_all: false })
    }

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
            "edit"   => { let (p, _) = self.write_parts(); json!({"name": "edit_file",      "input": {"path": p}}) }
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
                        self.actions.push(ParsedAction::new(name, raw.trim_matches('\n')));
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
