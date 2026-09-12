//! Skill Layer.
//!
//! - [`SkillRegistry`] / [`Skill`] — load, match and index `.md`/`.yaml` skill files
//! - [`runner`]     — execute MCP-mode skills via sidecar processes
//! - [`cache`]      — pull and cache skill files from HTTP/local sources
//! - [`fun`] — `iota-fun` stdio MCP server (multi-language runners)

pub mod cache;
pub mod fun;
pub mod runner;

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::acp::AcpBackend;

#[derive(Debug, Clone, Serialize)]
pub struct SkillRegistry {
    roots: Vec<PathBuf>,
    skills: BTreeMap<String, Skill>,
    diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Skill {
    pub metadata: SkillMetadata,
    pub body: String,
    pub path: PathBuf,
    pub priority: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    #[serde(default)]
    pub version: Option<serde_yaml::Value>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub backends: Vec<String>,
    #[serde(default)]
    pub execution: SkillExecution,
    #[serde(default)]
    pub output: SkillOutput,
    #[serde(default, rename = "failurePolicy")]
    pub failure_policy: Option<String>,
}

impl SkillMetadata {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            anyhow::bail!("Skill name cannot be empty");
        }
        for trigger in &self.triggers {
            if trigger.trim().is_empty() {
                anyhow::bail!("Trigger in skill '{}' cannot be empty", self.name);
            }
        }
        if self.execution.tools.len() > MAX_SKILL_TOOLS {
            anyhow::bail!(
                "Skill '{}' declares {} tools; maximum is {}",
                self.name,
                self.execution.tools.len(),
                MAX_SKILL_TOOLS
            );
        }
        let server = self.execution.server.as_deref().unwrap_or("iota-fun");
        if self.execution.mode.is_mcp() && !ALLOWED_SKILL_SERVERS.contains(&server) {
            anyhow::bail!(
                "Skill '{}' uses unsupported MCP server '{}'; allowed servers: {}",
                self.name,
                server,
                ALLOWED_SKILL_SERVERS.join(", ")
            );
        }
        if !self.execution.mode.is_mcp()
            && (self.execution.server.is_some() || !self.execution.tools.is_empty())
        {
            anyhow::bail!(
                "Advisory skill '{}' cannot declare an MCP server or tools",
                self.name
            );
        }
        let mut seen_tools = BTreeSet::new();
        for tool in &self.execution.tools {
            let tool_name = tool.name.trim();
            if tool_name.is_empty() {
                anyhow::bail!("Tool in skill '{}' cannot be empty", self.name);
            }
            let valid_namespace = match server {
                "iota-fun" => tool_name.starts_with("fun."),
                "iota-context" => tool_name.starts_with("iota_"),
                _ => false,
            };
            if !valid_namespace {
                anyhow::bail!(
                    "Tool '{}' is not valid for trusted server '{}'",
                    tool_name,
                    server
                );
            }
            if !seen_tools.insert(tool_name.to_string()) {
                anyhow::bail!("Duplicate tool '{}' in skill '{}'", tool_name, self.name);
            }
        }
        Ok(())
    }
}

const MAX_SKILL_TOOLS: usize = 32;
const ALLOWED_SKILL_SERVERS: &[&str] = &["iota-fun", "iota-context"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl SkillTool {
    pub fn label(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExecution {
    #[serde(default = "default_execution_mode")]
    pub mode: SkillExecutionMode,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default, deserialize_with = "deserialize_skill_tools")]
    pub tools: Vec<SkillTool>,
}

impl Default for SkillExecution {
    fn default() -> Self {
        Self {
            mode: default_execution_mode(),
            server: None,
            parallel: false,
            tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SkillExecutionMode {
    #[default]
    Advisory,
    Mcp,
}

impl SkillExecutionMode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Advisory => "advisory",
            Self::Mcp => "mcp",
        }
    }

    pub fn is_mcp(&self) -> bool {
        matches!(self, Self::Mcp)
    }
}

impl Serialize for SkillExecutionMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SkillExecutionMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.trim().to_ascii_lowercase().as_str() {
            "advisory" => Self::Advisory,
            "mcp" => Self::Mcp,
            _ => {
                return Err(serde::de::Error::custom(format!(
                    "invalid skill execution mode '{}'; expected advisory or mcp",
                    value
                )));
            }
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillOutput {
    #[serde(default)]
    pub template: String,
}

impl SkillRegistry {
    pub fn load(workspace: &Path, configured_roots: &[PathBuf]) -> Self {
        Self::load_from_roots(skill_roots(workspace, configured_roots))
    }

    pub fn load_cached(
        workspace: &Path,
        configured_roots: &[PathBuf],
        cache: &mut SkillCache,
    ) -> Self {
        let roots = skill_roots(workspace, configured_roots);
        let signature = roots_signature(&roots);
        if let Some((cached_signature, registry)) = cache.entry.as_ref()
            && cached_signature == &signature
        {
            return registry.clone();
        }

        let registry = Self::load_from_roots(roots);
        cache.entry = Some((signature, registry.clone()));
        registry
    }

    fn load_from_roots(roots: Vec<PathBuf>) -> Self {
        let mut registry = Self {
            roots,
            skills: BTreeMap::new(),
            diagnostics: Vec::new(),
        };
        registry.reload();
        registry
    }
}

fn skill_roots(workspace: &Path, configured_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(workspace.join("skills"));
    roots.push(workspace.join(".iota").join("skills"));
    roots.extend(configured_roots.iter().cloned());
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".i6").join("skills"));
    }
    roots
}

impl SkillRegistry {
    pub fn reload(&mut self) {
        self.skills.clear();
        self.diagnostics.clear();
        let roots = self.roots.clone();
        // Iterate roots in *reverse* priority order so that higher-priority roots
        // (lower index, e.g. workspace at index 0) are processed last and their
        // entries win on BTreeMap::insert collision.  The `priority` field stored
        // on each Skill reflects the original index (0 = highest priority).
        for (priority, root) in roots.iter().enumerate().rev() {
            if !root.exists() {
                continue;
            }
            if let Err(err) = self.load_root(root, priority) {
                self.diagnostics
                    .push(format!("{}: {}", root.display(), err));
            }
        }
        self.load_core_builtins();
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn compatible_skills(&self, backend: AcpBackend) -> Vec<&Skill> {
        self.skills
            .values()
            .filter(|skill| skill.supports_backend(backend))
            .collect()
    }

    pub fn skill_index(&self, backend: AcpBackend, budget: usize) -> String {
        let mut output = String::new();
        for skill in self.compatible_skills(backend) {
            let summary = skill
                .metadata
                .summary
                .as_deref()
                .or(skill.metadata.description.as_deref())
                .unwrap_or("");
            let line = format!("- {}: {}\n", skill.metadata.name, summary.trim());
            if output.len() + line.len() > budget {
                break;
            }
            output.push_str(&line);
        }
        output
    }

    pub fn match_skill(&self, backend: AcpBackend, prompt: &str) -> Option<&Skill> {
        let normalized = prompt.to_lowercase();
        self.compatible_skills(backend).into_iter().find(|skill| {
            skill
                .metadata
                .triggers
                .iter()
                .any(|trigger| normalized.contains(&trigger.to_lowercase()))
        })
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    fn load_root(&mut self, root: &Path, priority: usize) -> Result<()> {
        let mut files = collect_skill_files(root)?;
        files.sort();
        let mut seen_in_root = BTreeSet::new();
        for path in files {
            match parse_skill_file(&path, priority) {
                Ok(skill) => {
                    if !seen_in_root.insert(skill.metadata.name.clone()) {
                        self.diagnostics.push(format!(
                            "duplicate skill '{}' in {}; kept first sorted item",
                            skill.metadata.name,
                            root.display()
                        ));
                        continue;
                    }
                    self.skills.insert(skill.metadata.name.clone(), skill);
                }
                Err(err) => self
                    .diagnostics
                    .push(format!("{}: {}", path.display(), err)),
            }
        }
        Ok(())
    }

    fn load_core_builtins(&mut self) {
        match parse_core_skill(
            CORE_MEMORY_TAXONOMY_SKILL,
            PathBuf::from(CORE_MEMORY_TAXONOMY_SKILL_PATH),
        ) {
            Ok(skill) => {
                self.skills.insert(skill.metadata.name.clone(), skill);
            }
            Err(err) => {
                self.diagnostics
                    .push(format!("core builtin iota-memory-taxonomy: {}", err));
            }
        }
    }
}

impl Skill {
    pub fn supports_backend(&self, backend: AcpBackend) -> bool {
        self.metadata.backends.is_empty()
            || self
                .metadata
                .backends
                .iter()
                .any(|name| AcpBackend::parse(name).is_ok_and(|value| value == backend))
    }
}

pub fn render_template(skill: &Skill, prompt: &str) -> String {
    let template = skill.metadata.output.template.trim();
    if template.is_empty() {
        return skill.body.clone();
    }
    template
        .replace("{{prompt}}", prompt)
        .replace("{{skill.name}}", &skill.metadata.name)
}

fn collect_skill_files(root: &Path) -> Result<Vec<PathBuf>> {
    collect_skill_files_depth(root, 0)
}

const MAX_SKILL_DIR_DEPTH: usize = 8;

const CORE_MEMORY_TAXONOMY_SKILL_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/skill/core/iota-memory-taxonomy/SKILL.md"
);

fn collect_skill_files_depth(root: &Path, depth: usize) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(root).with_context(|| format!("Failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_SKILL_DIR_DEPTH {
                files.extend(collect_skill_files_depth(&path, depth + 1)?);
            }
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| matches!(ext, "md" | "yaml" | "yml"))
        {
            files.push(path);
        }
    }
    Ok(files)
}

fn parse_skill_file(path: &Path, priority: usize) -> Result<Skill> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read skill {}", path.display()))?;
    let (frontmatter, body) = split_frontmatter(&content)
        .with_context(|| format!("Skill {} is missing YAML frontmatter", path.display()))?;
    let metadata: SkillMetadata = serde_yaml::from_str(frontmatter)
        .with_context(|| format!("Invalid skill YAML in {}", path.display()))?;
    metadata.validate()?;
    Ok(Skill {
        metadata,
        body: body.trim().to_string(),
        path: path.to_path_buf(),
        priority,
    })
}

fn parse_core_skill(content: &str, path: PathBuf) -> Result<Skill> {
    let (frontmatter, body) = split_frontmatter(content)
        .with_context(|| format!("Core skill {} is missing YAML frontmatter", path.display()))?;
    let metadata: SkillMetadata = serde_yaml::from_str(frontmatter)
        .with_context(|| format!("Invalid core skill YAML in {}", path.display()))?;
    metadata.validate()?;
    Ok(Skill {
        metadata,
        body: body.trim().to_string(),
        path,
        priority: 0,
    })
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let (frontmatter, after) = rest.split_at(end);
    let body = after.strip_prefix("\n---").unwrap_or(after);
    let body = body.strip_prefix('\n').unwrap_or(body);
    Some((frontmatter, body))
}

fn default_execution_mode() -> SkillExecutionMode {
    SkillExecutionMode::Advisory
}

fn deserialize_skill_tools<'de, D>(deserializer: D) -> Result<Vec<SkillTool>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<serde_yaml::Value>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|value| match value {
            serde_yaml::Value::String(name) => Ok(SkillTool { name, alias: None }),
            serde_yaml::Value::Mapping(mapping) => {
                let name = mapping
                    .get(serde_yaml::Value::String("name".to_string()))
                    .and_then(serde_yaml::Value::as_str)
                    .ok_or_else(|| serde::de::Error::custom("skill tool object requires name"))?
                    .to_string();
                let alias = mapping
                    .get(serde_yaml::Value::String("as".to_string()))
                    .and_then(serde_yaml::Value::as_str)
                    .map(str::to_string);
                Ok(SkillTool { name, alias })
            }
            _ => Err(serde::de::Error::custom(
                "skill tools must be strings or {name, as} objects",
            )),
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct SkillCache {
    entry: Option<(String, SkillRegistry)>,
}

fn roots_signature(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| format!("{}:{}", root.display(), latest_mtime(root).unwrap_or(0)))
        .collect::<Vec<_>>()
        .join("|")
}

fn latest_mtime(path: &Path) -> Option<u128> {
    latest_mtime_depth(path, 0)
}

fn latest_mtime_depth(path: &Path, depth: usize) -> Option<u128> {
    if !path.exists() {
        return Some(0);
    }
    let mut latest = path
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_ms)
        .unwrap_or(0);
    if path.is_dir()
        && depth < MAX_SKILL_DIR_DEPTH
        && let Ok(entries) = fs::read_dir(path)
    {
        for entry in entries.flatten() {
            latest = latest.max(latest_mtime_depth(&entry.path(), depth + 1).unwrap_or(0));
        }
    }
    Some(latest)
}

fn system_time_ms(time: SystemTime) -> Option<u128> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

const CORE_MEMORY_TAXONOMY_SKILL: &str = include_str!("core/iota-memory-taxonomy/SKILL.md");

#[cfg(test)]
#[path = "skill_tests.rs"]
mod tests;
