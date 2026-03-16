use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// Metadata for a prompt entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptEntry {
    /// The prompt template text with `{{var}}` placeholders.
    pub template: String,
    /// Human-readable description of what this prompt does.
    pub description: String,
    /// Category: "chat" or "document".
    pub category: String,
    /// Whether this was loaded from a file (vs database override).
    pub source: PromptSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PromptSource {
    /// Default from embedded/file.
    Default,
    /// Overridden via API/database.
    Override,
}

/// Thread-safe registry of prompt templates.
///
/// Loading priority: database overrides > markdown files > hardcoded defaults.
/// Agents call `render_or_default(key, hardcoded, vars)` which checks the
/// registry first and falls back to the hardcoded string if not found.
pub struct PromptRegistry {
    prompts: RwLock<HashMap<String, PromptEntry>>,
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptRegistry {
    pub fn new() -> Self {
        Self {
            prompts: RwLock::new(HashMap::new()),
        }
    }

    /// Load prompt templates from a directory structure.
    ///
    /// Expected layout:
    /// ```text
    /// prompts/
    ///   chat/
    ///     rag_engine.md
    ///     query_analyzer.md
    ///   document/
    ///     analyzer.md
    ///     converter.md
    /// ```
    ///
    /// Keys are derived as `{subdir}.{filename}`, e.g. `chat.rag_engine`.
    pub fn load_from_dir(&self, dir: &Path) -> std::io::Result<usize> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        let mut prompts = self.prompts.write().unwrap();

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let category = path.file_name().unwrap().to_string_lossy().to_string();

                for sub_entry in std::fs::read_dir(&path)? {
                    let sub_entry = sub_entry?;
                    let sub_path = sub_entry.path();

                    if sub_path.extension().is_some_and(|e| e == "md") {
                        let stem = sub_path.file_stem().unwrap().to_string_lossy().to_string();
                        let key = format!("{category}.{stem}");
                        let content = std::fs::read_to_string(&sub_path)?;

                        // Parse optional frontmatter (---\ndescription: ...\n---)
                        let (template, description) = parse_frontmatter(&content);

                        prompts.insert(
                            key,
                            PromptEntry {
                                template,
                                description,
                                category: category.clone(),
                                source: PromptSource::Default,
                            },
                        );
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Get a prompt template by key.
    pub fn get(&self, key: &str) -> Option<PromptEntry> {
        self.prompts.read().unwrap().get(key).cloned()
    }

    /// Get just the template text.
    pub fn get_template(&self, key: &str) -> Option<String> {
        self.prompts
            .read()
            .unwrap()
            .get(key)
            .map(|e| e.template.clone())
    }

    /// Set a prompt (override). Used by API/database.
    pub fn set(&self, key: &str, template: String, description: String, category: String) {
        self.prompts.write().unwrap().insert(
            key.to_string(),
            PromptEntry {
                template,
                description,
                category,
                source: PromptSource::Override,
            },
        );
    }

    /// Delete an override, reverting to the file/hardcoded default.
    pub fn delete_override(&self, key: &str) -> bool {
        let mut prompts = self.prompts.write().unwrap();
        if let Some(entry) = prompts.get(key) {
            if entry.source == PromptSource::Override {
                prompts.remove(key);
                return true;
            }
        }
        false
    }

    /// List all prompts.
    pub fn list(&self) -> Vec<(String, PromptEntry)> {
        let prompts = self.prompts.read().unwrap();
        let mut entries: Vec<_> = prompts
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    /// Render a prompt template with variable substitution.
    /// Returns `None` if the key is not in the registry.
    pub fn render(&self, key: &str, vars: &[(&str, &str)]) -> Option<String> {
        self.get_template(key)
            .map(|template| render_template(&template, vars))
    }

    /// Render a prompt from the registry, falling back to `default` if not found.
    ///
    /// This is the primary method agents should use:
    /// ```ignore
    /// let prompt = registry.render_or_default(
    ///     "chat.rag_engine",
    ///     "You are ThaiRAG...", // hardcoded default
    ///     &[("context", &context_text)],
    /// );
    /// ```
    pub fn render_or_default(&self, key: &str, default: &str, vars: &[(&str, &str)]) -> String {
        let template = self
            .get_template(key)
            .unwrap_or_else(|| default.to_string());
        render_template(&template, vars)
    }
}

/// Render a template by replacing `{{var}}` placeholders with values.
pub fn render_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let mut pattern = String::with_capacity(key.len() + 4);
        pattern.push_str("{{");
        pattern.push_str(key);
        pattern.push_str("}}");
        result = result.replace(&pattern, value);
    }
    result
}

/// Parse optional YAML-like frontmatter from a markdown prompt file.
///
/// Format:
/// ```markdown
/// ---
/// description: What this prompt does
/// ---
/// Actual prompt template here...
/// ```
fn parse_frontmatter(content: &str) -> (String, String) {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return (trimmed.to_string(), String::new());
    }

    // Find the closing ---
    if let Some(end) = trimmed[3..].find("---") {
        let frontmatter = &trimmed[3..3 + end];
        let template = trimmed[3 + end + 3..].trim().to_string();

        // Extract description from frontmatter
        let description = frontmatter
            .lines()
            .find_map(|line| {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("description:") {
                    Some(rest.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        (template, description)
    } else {
        (trimmed.to_string(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_template() {
        let template = "Hello {{name}}, your score is {{score}}.";
        let result = render_template(template, &[("name", "Alice"), ("score", "95")]);
        assert_eq!(result, "Hello Alice, your score is 95.");
    }

    #[test]
    fn test_render_with_json() {
        // JSON braces should NOT be replaced
        let template = r#"Return JSON: {"pass": true, "score": {{threshold}}}
Query: {{query}}"#;
        let result = render_template(template, &[("threshold", "0.8"), ("query", "test")]);
        assert_eq!(
            result,
            r#"Return JSON: {"pass": true, "score": 0.8}
Query: test"#
        );
    }

    #[test]
    fn test_render_or_default_with_fallback() {
        let registry = PromptRegistry::new();
        let result = registry.render_or_default(
            "nonexistent.key",
            "Default prompt for {{name}}",
            &[("name", "test")],
        );
        assert_eq!(result, "Default prompt for test");
    }

    #[test]
    fn test_render_or_default_with_override() {
        let registry = PromptRegistry::new();
        registry.set(
            "test.key",
            "Custom prompt for {{name}}!".to_string(),
            "Test".to_string(),
            "test".to_string(),
        );
        let result = registry.render_or_default(
            "test.key",
            "Default prompt for {{name}}",
            &[("name", "override")],
        );
        assert_eq!(result, "Custom prompt for override!");
    }

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\ndescription: Test prompt\n---\nHello {{name}}";
        let (template, desc) = parse_frontmatter(content);
        assert_eq!(template, "Hello {{name}}");
        assert_eq!(desc, "Test prompt");
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let content = "Just a prompt {{var}}";
        let (template, desc) = parse_frontmatter(content);
        assert_eq!(template, "Just a prompt {{var}}");
        assert_eq!(desc, "");
    }
}
