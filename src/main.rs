use std::{env, error::Error, path::PathBuf, time::Duration};

use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio::{process::Command, time::timeout};
use tracing_subscriber::EnvFilter;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const CLI_PATH_ENV: &str = "GITQUARRY_CLI_PATH";
const TIMEOUT_ENV: &str = "GITQUARRY_MCP_TIMEOUT_MS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Json,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandSpec {
    args: Vec<String>,
    output_mode: OutputMode,
}

#[derive(Debug, Clone)]
struct CliRunner {
    cli_path: PathBuf,
    timeout: Duration,
}

#[derive(Debug, Clone, PartialEq)]
enum CommandOutput {
    Json(Value),
    Text(String),
}

#[derive(Debug, Error)]
enum RunnerError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("failed to start `{path}`: {message}")]
    Spawn { path: String, message: String },
    #[error("`{path}` timed out after {timeout_ms}ms")]
    Timeout { path: String, timeout_ms: u64 },
    #[error("{message}")]
    CommandFailed { message: String },
    #[error("failed to parse CLI JSON output: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum RetrievalMode {
    Native,
    Discover,
}

impl RetrievalMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Discover => "discover",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum RankMode {
    Native,
    Query,
    Activity,
    Quality,
    Blended,
}

impl RankMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Query => "query",
            Self::Activity => "activity",
            Self::Quality => "quality",
            Self::Blended => "blended",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum SearchSort {
    BestMatch,
    Stars,
    Updated,
}

impl SearchSort {
    fn as_str(self) -> &'static str {
        match self {
            Self::BestMatch => "best-match",
            Self::Stars => "stars",
            Self::Updated => "updated",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum DiscoveryDepth {
    Quick,
    Balanced,
    Deep,
}

impl DiscoveryDepth {
    fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Balanced => "balanced",
            Self::Deep => "deep",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum BoolFlag {
    True,
    False,
}

impl BoolFlag {
    fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ForkMode {
    False,
    True,
    Only,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum PatternMode {
    Literal,
    Regex,
}

impl PatternMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Regex => "regex",
        }
    }
}

impl ForkMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::False => "false",
            Self::True => "true",
            Self::Only => "only",
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq)]
struct SearchArgs {
    /// Optional GitHub host override. Accepts github.com, a full URL, or an API base.
    #[serde(default)]
    host: Option<String>,
    /// Free-text query. Omit only when using discover mode with structured filters.
    #[serde(default)]
    query: Option<String>,
    /// Retrieval mode.
    #[serde(default)]
    mode: Option<RetrievalMode>,
    /// Ranking mode. Non-native ranks require discover mode upstream.
    #[serde(default)]
    rank: Option<RankMode>,
    /// Native GitHub-like sort order.
    #[serde(default)]
    sort: Option<SearchSort>,
    /// Discover-mode depth.
    #[serde(default)]
    depth: Option<DiscoveryDepth>,
    /// Maximum number of repositories to return.
    #[serde(default)]
    limit: Option<u32>,
    /// Restrict search to one user.
    #[serde(default)]
    user: Option<String>,
    /// Restrict search to one organization.
    #[serde(default)]
    org: Option<String>,
    /// Filter archived repositories.
    #[serde(default)]
    archived: Option<BoolFlag>,
    /// Filter template repositories.
    #[serde(default)]
    template: Option<BoolFlag>,
    /// Filter fork state.
    #[serde(default)]
    fork: Option<ForkMode>,
    /// Require one language. Repeats use AND semantics.
    #[serde(default)]
    language: Vec<String>,
    /// Require one topic. Repeats use AND semantics.
    #[serde(default)]
    topic: Vec<String>,
    /// Require one license. Repeats use AND semantics.
    #[serde(default)]
    license: Vec<String>,
    /// Minimum stars.
    #[serde(default)]
    min_stars: Option<u64>,
    /// Maximum stars.
    #[serde(default)]
    max_stars: Option<u64>,
    /// Minimum forks.
    #[serde(default)]
    min_forks: Option<u64>,
    /// Maximum forks.
    #[serde(default)]
    max_forks: Option<u64>,
    /// Minimum repository size in KB.
    #[serde(default)]
    min_size: Option<u64>,
    /// Maximum repository size in KB.
    #[serde(default)]
    max_size: Option<u64>,
    /// Created-on-or-after date in YYYY-MM-DD.
    #[serde(default)]
    created_after: Option<String>,
    /// Created-on-or-before date in YYYY-MM-DD.
    #[serde(default)]
    created_before: Option<String>,
    /// Updated-on-or-after date in YYYY-MM-DD.
    #[serde(default)]
    updated_after: Option<String>,
    /// Updated-on-or-before date in YYYY-MM-DD.
    #[serde(default)]
    updated_before: Option<String>,
    /// Pushed-on-or-after date in YYYY-MM-DD.
    #[serde(default)]
    pushed_after: Option<String>,
    /// Pushed-on-or-before date in YYYY-MM-DD.
    #[serde(default)]
    pushed_before: Option<String>,
    /// Require created recency like 30d, 12h, or 1y.
    #[serde(default)]
    created_within: Option<String>,
    /// Require updated recency like 30d, 12h, or 1y.
    #[serde(default)]
    updated_within: Option<String>,
    /// Require push recency like 30d, 12h, or 1y.
    #[serde(default)]
    pushed_within: Option<String>,
    /// Include README enrichment for the candidate window.
    #[serde(default)]
    readme: Option<bool>,
    /// Include ranking explanations in enhanced modes.
    #[serde(default)]
    explain: Option<bool>,
    /// Blended query weight in the range 0.0..=3.0.
    #[serde(default)]
    weight_query: Option<f64>,
    /// Blended activity weight in the range 0.0..=3.0.
    #[serde(default)]
    weight_activity: Option<f64>,
    /// Blended quality weight in the range 0.0..=3.0.
    #[serde(default)]
    weight_quality: Option<f64>,
    /// Worker count for discover-mode enrichment.
    #[serde(default)]
    concurrency: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
struct InspectArgs {
    /// Optional GitHub host override. Accepts github.com, a full URL, or an API base.
    #[serde(default)]
    host: Option<String>,
    /// Explicit repository identifier in owner/repo form.
    repository: String,
    /// Include the repository README in the output.
    #[serde(default)]
    readme: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
struct TreeArgs {
    /// Optional GitHub host override. Accepts github.com, a full URL, or an API base.
    #[serde(default)]
    host: Option<String>,
    /// Explicit repository identifier in owner/repo form.
    repository: String,
    /// Git ref to inspect. Defaults to the repository default branch.
    #[serde(default)]
    reference: Option<String>,
    /// Only show paths matching these glob patterns.
    #[serde(default)]
    paths: Vec<String>,
    /// Only show paths containing this text.
    #[serde(default)]
    contains: Option<String>,
    /// Maximum path depth to return, where root entries are depth 1.
    #[serde(default)]
    depth: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
struct CodeArgs {
    /// Optional GitHub host override. Accepts github.com, a full URL, or an API base.
    #[serde(default)]
    host: Option<String>,
    /// Explicit repository identifier in owner/repo form.
    repository: String,
    /// Text or regex pattern to search for.
    pattern: String,
    /// Git ref to inspect. Defaults to the repository default branch.
    #[serde(default)]
    reference: Option<String>,
    /// Restrict searched files to these glob patterns.
    #[serde(default)]
    paths: Vec<String>,
    /// Treat the pattern as literal text or a Rust regex.
    #[serde(default)]
    mode: Option<PatternMode>,
    /// Lines of context to include before and after each match.
    #[serde(default)]
    context: Option<u32>,
    /// Maximum number of matches to return.
    #[serde(default)]
    limit: Option<u32>,
    /// Maximum file size to fetch in bytes.
    #[serde(default)]
    max_file_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
struct HostArgs {
    /// Optional GitHub host override. Accepts github.com, a full URL, or an API base.
    #[serde(default)]
    host: Option<String>,
}

#[derive(Clone)]
struct GitquarryServer {
    runner: CliRunner,
    #[allow(dead_code)]
    tool_router: ToolRouter<GitquarryServer>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let runner = CliRunner::from_env()?;
    let service = GitquarryServer::new(runner).serve(stdio()).await?;

    tracing::info!("gitquarry-mcp started");
    service.waiting().await?;
    Ok(())
}

impl CliRunner {
    fn from_env() -> Result<Self, RunnerError> {
        let cli_path = env::var(CLI_PATH_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("gitquarry"));
        let timeout = match env::var(TIMEOUT_ENV) {
            Ok(raw) => {
                let value = raw.parse::<u64>().map_err(|_| {
                    RunnerError::Config(format!(
                        "{TIMEOUT_ENV} must be a positive integer in milliseconds"
                    ))
                })?;
                if value == 0 {
                    return Err(RunnerError::Config(format!(
                        "{TIMEOUT_ENV} must be greater than 0"
                    )));
                }
                Duration::from_millis(value)
            }
            Err(_) => Duration::from_millis(DEFAULT_TIMEOUT_MS),
        };

        Ok(Self { cli_path, timeout })
    }

    #[cfg(test)]
    fn new(cli_path: PathBuf, timeout: Duration) -> Self {
        Self { cli_path, timeout }
    }

    async fn run(&self, spec: CommandSpec) -> Result<CommandOutput, RunnerError> {
        let path_display = self.cli_path.display().to_string();
        let mut command = Command::new(&self.cli_path);
        command
            .args(&spec.args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let child = command.spawn().map_err(|error| RunnerError::Spawn {
            path: path_display.clone(),
            message: error.to_string(),
        })?;

        let output = timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| RunnerError::Timeout {
                path: path_display.clone(),
                timeout_ms: self.timeout.as_millis() as u64,
            })?
            .map_err(|error| RunnerError::Spawn {
                path: path_display.clone(),
                message: error.to_string(),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string();
        let stderr = String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_string();

        if !output.status.success() {
            let message = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                match output.status.code() {
                    Some(code) => format!("`{path_display}` exited with status {code}"),
                    None => format!("`{path_display}` terminated by signal"),
                }
            };
            return Err(RunnerError::CommandFailed { message });
        }

        match spec.output_mode {
            OutputMode::Json => {
                let value = serde_json::from_str(&stdout)
                    .map_err(|error| RunnerError::Parse(error.to_string()))?;
                Ok(CommandOutput::Json(value))
            }
            OutputMode::Text => Ok(CommandOutput::Text(stdout)),
        }
    }
}

impl GitquarryServer {
    fn new(runner: CliRunner) -> Self {
        Self {
            runner,
            tool_router: Self::tool_router(),
        }
    }

    async fn execute(&self, spec: CommandSpec) -> CallToolResult {
        match self.runner.run(spec).await {
            Ok(CommandOutput::Json(value)) => json_tool_result(value),
            Ok(CommandOutput::Text(text)) => CallToolResult::success(vec![Content::text(text)]),
            Err(error) => CallToolResult::error(vec![Content::text(error.to_string())]),
        }
    }
}

#[tool_router]
impl GitquarryServer {
    #[tool(
        description = "Search GitHub repositories through gitquarry and return structured JSON."
    )]
    async fn gitquarry_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.execute(search(args)).await)
    }

    #[tool(
        description = "Inspect one explicit owner/repo through gitquarry and return structured JSON."
    )]
    async fn gitquarry_inspect(
        &self,
        Parameters(args): Parameters<InspectArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.execute(inspect(args)).await)
    }

    #[tool(
        description = "Fetch a repository tree through gitquarry without cloning and return structured JSON."
    )]
    async fn gitquarry_tree(
        &self,
        Parameters(args): Parameters<TreeArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.execute(tree(args)).await)
    }

    #[tool(
        description = "Search repository code through gitquarry without cloning and return structured JSON."
    )]
    async fn gitquarry_code(
        &self,
        Parameters(args): Parameters<CodeArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.execute(code(args)).await)
    }

    #[tool(description = "Show whether gitquarry has a saved token for the effective host.")]
    async fn gitquarry_auth_status(
        &self,
        Parameters(args): Parameters<HostArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.execute(auth_status(args)).await)
    }

    #[tool(description = "Delete the saved gitquarry token for the effective host.")]
    async fn gitquarry_auth_logout(
        &self,
        Parameters(args): Parameters<HostArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.execute(auth_logout(args)).await)
    }

    #[tool(description = "Print the effective gitquarry config path.")]
    async fn gitquarry_config_path(&self) -> Result<CallToolResult, McpError> {
        Ok(self.execute(config_path()).await)
    }

    #[tool(description = "Print the effective gitquarry config payload as JSON.")]
    async fn gitquarry_config_show(&self) -> Result<CallToolResult, McpError> {
        Ok(self.execute(config_show()).await)
    }

    #[tool(description = "Print the wrapped gitquarry version.")]
    async fn gitquarry_version(&self) -> Result<CallToolResult, McpError> {
        Ok(self.execute(version()).await)
    }
}

#[tool_handler]
impl ServerHandler for GitquarryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_instructions(
                "This server wraps the external `gitquarry` CLI. Search and inspect tools force \
                 JSON output and disable progress noise so MCP clients receive clean structured \
                 results. Configure GitHub credentials outside MCP via gitquarry env vars or by \
                 running `gitquarry auth login` manually in a terminal."
                    .to_string(),
            )
    }
}

fn search(args: SearchArgs) -> CommandSpec {
    let mut argv = Vec::new();
    push_host(&mut argv, args.host);
    argv.push("search".to_string());
    if let Some(query) = args.query {
        argv.push(query);
    }
    push_opt_value(
        &mut argv,
        "--mode",
        args.mode.map(|value| value.as_str().to_string()),
    );
    push_opt_value(
        &mut argv,
        "--rank",
        args.rank.map(|value| value.as_str().to_string()),
    );
    push_opt_value(
        &mut argv,
        "--sort",
        args.sort.map(|value| value.as_str().to_string()),
    );
    push_opt_value(
        &mut argv,
        "--depth",
        args.depth.map(|value| value.as_str().to_string()),
    );
    push_opt_u32(&mut argv, "--limit", args.limit);
    push_opt_value(&mut argv, "--user", args.user);
    push_opt_value(&mut argv, "--org", args.org);
    push_opt_value(
        &mut argv,
        "--archived",
        args.archived.map(|value| value.as_str().to_string()),
    );
    push_opt_value(
        &mut argv,
        "--template",
        args.template.map(|value| value.as_str().to_string()),
    );
    push_opt_value(
        &mut argv,
        "--fork",
        args.fork.map(|value| value.as_str().to_string()),
    );
    push_repeat_values(&mut argv, "--language", args.language);
    push_repeat_values(&mut argv, "--topic", args.topic);
    push_repeat_values(&mut argv, "--license", args.license);
    push_opt_u64(&mut argv, "--min-stars", args.min_stars);
    push_opt_u64(&mut argv, "--max-stars", args.max_stars);
    push_opt_u64(&mut argv, "--min-forks", args.min_forks);
    push_opt_u64(&mut argv, "--max-forks", args.max_forks);
    push_opt_u64(&mut argv, "--min-size", args.min_size);
    push_opt_u64(&mut argv, "--max-size", args.max_size);
    push_opt_value(&mut argv, "--created-after", args.created_after);
    push_opt_value(&mut argv, "--created-before", args.created_before);
    push_opt_value(&mut argv, "--updated-after", args.updated_after);
    push_opt_value(&mut argv, "--updated-before", args.updated_before);
    push_opt_value(&mut argv, "--pushed-after", args.pushed_after);
    push_opt_value(&mut argv, "--pushed-before", args.pushed_before);
    push_opt_value(&mut argv, "--created-within", args.created_within);
    push_opt_value(&mut argv, "--updated-within", args.updated_within);
    push_opt_value(&mut argv, "--pushed-within", args.pushed_within);
    push_flag_if_true(&mut argv, "--readme", args.readme);
    push_flag_if_true(&mut argv, "--explain", args.explain);
    push_opt_f64(&mut argv, "--weight-query", args.weight_query);
    push_opt_f64(&mut argv, "--weight-activity", args.weight_activity);
    push_opt_f64(&mut argv, "--weight-quality", args.weight_quality);
    push_opt_u32(&mut argv, "--concurrency", args.concurrency);
    // MCP needs machine-readable stdout and no progress chatter on stderr.
    argv.push("--format".to_string());
    argv.push("json".to_string());
    argv.push("--progress".to_string());
    argv.push("off".to_string());

    CommandSpec {
        args: argv,
        output_mode: OutputMode::Json,
    }
}

fn inspect(args: InspectArgs) -> CommandSpec {
    let mut argv = Vec::new();
    push_host(&mut argv, args.host);
    argv.push("inspect".to_string());
    argv.push(args.repository);
    push_flag_if_true(&mut argv, "--readme", args.readme);
    argv.push("--format".to_string());
    argv.push("json".to_string());
    argv.push("--progress".to_string());
    argv.push("off".to_string());

    CommandSpec {
        args: argv,
        output_mode: OutputMode::Json,
    }
}

fn tree(args: TreeArgs) -> CommandSpec {
    let mut argv = Vec::new();
    push_host(&mut argv, args.host);
    argv.push("tree".to_string());
    argv.push(args.repository);
    push_opt_value(&mut argv, "--reference", args.reference);
    push_repeat_values(&mut argv, "--path", args.paths);
    push_opt_value(&mut argv, "--contains", args.contains);
    push_opt_u32(&mut argv, "--depth", args.depth);
    argv.push("--format".to_string());
    argv.push("json".to_string());
    argv.push("--progress".to_string());
    argv.push("off".to_string());

    CommandSpec {
        args: argv,
        output_mode: OutputMode::Json,
    }
}

fn code(args: CodeArgs) -> CommandSpec {
    let mut argv = Vec::new();
    push_host(&mut argv, args.host);
    argv.push("code".to_string());
    argv.push(args.repository);
    argv.push(args.pattern);
    push_opt_value(&mut argv, "--reference", args.reference);
    push_repeat_values(&mut argv, "--path", args.paths);
    push_opt_value(
        &mut argv,
        "--mode",
        args.mode.map(|value| value.as_str().to_string()),
    );
    push_opt_u32(&mut argv, "--context", args.context);
    push_opt_u32(&mut argv, "--limit", args.limit);
    push_opt_u64(&mut argv, "--max-file-bytes", args.max_file_bytes);
    argv.push("--format".to_string());
    argv.push("json".to_string());
    argv.push("--progress".to_string());
    argv.push("off".to_string());

    CommandSpec {
        args: argv,
        output_mode: OutputMode::Json,
    }
}

fn auth_status(args: HostArgs) -> CommandSpec {
    let mut argv = Vec::new();
    push_host(&mut argv, args.host);
    argv.push("auth".to_string());
    argv.push("status".to_string());
    CommandSpec {
        args: argv,
        output_mode: OutputMode::Text,
    }
}

fn auth_logout(args: HostArgs) -> CommandSpec {
    let mut argv = Vec::new();
    push_host(&mut argv, args.host);
    argv.push("auth".to_string());
    argv.push("logout".to_string());
    CommandSpec {
        args: argv,
        output_mode: OutputMode::Text,
    }
}

fn config_path() -> CommandSpec {
    CommandSpec {
        args: vec!["config".to_string(), "path".to_string()],
        output_mode: OutputMode::Text,
    }
}

fn config_show() -> CommandSpec {
    CommandSpec {
        args: vec!["config".to_string(), "show".to_string()],
        output_mode: OutputMode::Json,
    }
}

fn version() -> CommandSpec {
    CommandSpec {
        args: vec!["version".to_string()],
        output_mode: OutputMode::Text,
    }
}

fn push_host(argv: &mut Vec<String>, host: Option<String>) {
    push_opt_value(argv, "--host", host);
}

fn push_repeat_values(argv: &mut Vec<String>, flag: &str, values: Vec<String>) {
    for value in values {
        argv.push(flag.to_string());
        argv.push(value);
    }
}

fn push_opt_value(argv: &mut Vec<String>, flag: &str, value: Option<String>) {
    if let Some(value) = value {
        argv.push(flag.to_string());
        argv.push(value);
    }
}

fn push_flag_if_true(argv: &mut Vec<String>, flag: &str, value: Option<bool>) {
    if value.unwrap_or(false) {
        argv.push(flag.to_string());
    }
}

fn push_opt_u32(argv: &mut Vec<String>, flag: &str, value: Option<u32>) {
    if let Some(value) = value {
        argv.push(flag.to_string());
        argv.push(value.to_string());
    }
}

fn push_opt_u64(argv: &mut Vec<String>, flag: &str, value: Option<u64>) {
    if let Some(value) = value {
        argv.push(flag.to_string());
        argv.push(value.to_string());
    }
}

fn push_opt_f64(argv: &mut Vec<String>, flag: &str, value: Option<f64>) {
    if let Some(value) = value {
        argv.push(flag.to_string());
        argv.push(value.to_string());
    }
}

fn json_tool_result(value: Value) -> CallToolResult {
    let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    let mut result = CallToolResult::structured(value);
    result.content = vec![Content::text(pretty)];
    result
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    use tempfile::tempdir;

    use super::*;

    fn write_fixture(dir: &Path, body: &str, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).expect("fixture script should write");
        let mut perms = fs::metadata(&path)
            .expect("fixture metadata should exist")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("fixture should be executable");
        path
    }

    #[test]
    fn builds_search_args() {
        let spec = search(SearchArgs {
            host: Some("github.example.com".to_string()),
            query: Some("rust cli".to_string()),
            mode: Some(RetrievalMode::Discover),
            rank: Some(RankMode::Blended),
            sort: Some(SearchSort::Stars),
            depth: Some(DiscoveryDepth::Deep),
            limit: Some(5),
            user: Some("microck".to_string()),
            org: None,
            archived: Some(BoolFlag::False),
            template: None,
            fork: Some(ForkMode::Only),
            language: vec!["rust".to_string(), "typescript".to_string()],
            topic: vec!["mcp".to_string()],
            license: vec!["mit".to_string()],
            min_stars: Some(100),
            max_stars: None,
            min_forks: None,
            max_forks: None,
            min_size: None,
            max_size: None,
            created_after: None,
            created_before: None,
            updated_after: Some("2025-01-01".to_string()),
            updated_before: None,
            pushed_after: None,
            pushed_before: None,
            created_within: None,
            updated_within: Some("30d".to_string()),
            pushed_within: None,
            readme: Some(true),
            explain: Some(true),
            weight_query: Some(1.5),
            weight_activity: None,
            weight_quality: Some(0.8),
            concurrency: Some(4),
        });

        assert_eq!(
            spec.args,
            vec![
                "--host",
                "github.example.com",
                "search",
                "rust cli",
                "--mode",
                "discover",
                "--rank",
                "blended",
                "--sort",
                "stars",
                "--depth",
                "deep",
                "--limit",
                "5",
                "--user",
                "microck",
                "--archived",
                "false",
                "--fork",
                "only",
                "--language",
                "rust",
                "--language",
                "typescript",
                "--topic",
                "mcp",
                "--license",
                "mit",
                "--min-stars",
                "100",
                "--updated-after",
                "2025-01-01",
                "--updated-within",
                "30d",
                "--readme",
                "--explain",
                "--weight-query",
                "1.5",
                "--weight-quality",
                "0.8",
                "--concurrency",
                "4",
                "--format",
                "json",
                "--progress",
                "off",
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn builds_inspect_args() {
        let spec = inspect(InspectArgs {
            host: Some("github.com".to_string()),
            repository: "rust-lang/rust".to_string(),
            readme: Some(true),
        });

        assert_eq!(
            spec.args,
            vec![
                "--host",
                "github.com",
                "inspect",
                "rust-lang/rust",
                "--readme",
                "--format",
                "json",
                "--progress",
                "off",
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn builds_tree_args() {
        let spec = tree(TreeArgs {
            host: Some("github.com".to_string()),
            repository: "microck/gitquarry".to_string(),
            reference: Some("main".to_string()),
            paths: vec!["src/*".to_string(), "tests/*".to_string()],
            contains: Some(".rs".to_string()),
            depth: Some(2),
        });

        assert_eq!(
            spec.args,
            vec![
                "--host",
                "github.com",
                "tree",
                "microck/gitquarry",
                "--reference",
                "main",
                "--path",
                "src/*",
                "--path",
                "tests/*",
                "--contains",
                ".rs",
                "--depth",
                "2",
                "--format",
                "json",
                "--progress",
                "off",
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn builds_code_args() {
        let spec = code(CodeArgs {
            host: Some("github.com".to_string()),
            repository: "microck/gitquarry".to_string(),
            pattern: "fn main".to_string(),
            reference: Some("main".to_string()),
            paths: vec!["src/*.rs".to_string()],
            mode: Some(PatternMode::Regex),
            context: Some(2),
            limit: Some(25),
            max_file_bytes: Some(500_000),
        });

        assert_eq!(
            spec.args,
            vec![
                "--host",
                "github.com",
                "code",
                "microck/gitquarry",
                "fn main",
                "--reference",
                "main",
                "--path",
                "src/*.rs",
                "--mode",
                "regex",
                "--context",
                "2",
                "--limit",
                "25",
                "--max-file-bytes",
                "500000",
                "--format",
                "json",
                "--progress",
                "off",
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn parses_json_output() {
        let dir = tempdir().expect("tempdir");
        let script = write_fixture(
            dir.path(),
            "#!/usr/bin/env bash\nprintf '{\"ok\":true,\"args\":[\"%s\",\"%s\"]}\\n' \"$1\" \"$2\"\n",
            "gitquarry",
        );
        let runner = CliRunner::new(script, Duration::from_millis(500));

        let output = runner
            .run(CommandSpec {
                args: vec!["version".to_string(), "--json".to_string()],
                output_mode: OutputMode::Json,
            })
            .await
            .expect("json output should parse");

        assert_eq!(
            output,
            CommandOutput::Json(serde_json::json!({
                "ok": true,
                "args": ["version", "--json"]
            }))
        );
    }

    #[tokio::test]
    async fn surfaces_stderr_for_failures() {
        let dir = tempdir().expect("tempdir");
        let script = write_fixture(
            dir.path(),
            "#!/usr/bin/env bash\nprintf 'authentication error: invalid token\\n' >&2\nexit 1\n",
            "gitquarry",
        );
        let runner = CliRunner::new(script, Duration::from_millis(500));

        let error = runner
            .run(CommandSpec {
                args: vec!["search".to_string(), "rust".to_string()],
                output_mode: OutputMode::Json,
            })
            .await
            .expect_err("runner should return CLI failure");

        assert!(
            error
                .to_string()
                .contains("authentication error: invalid token")
        );
    }

    #[test]
    fn wraps_json_with_text_and_structured_content() {
        let value = serde_json::json!({ "items": ["a", "b"] });
        let result = json_tool_result(value.clone());

        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content, Some(value));
        assert_eq!(result.content.len(), 1);
    }
}
