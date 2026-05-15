# gitquarry-mcp

`gitquarry-mcp` is a small stdio MCP server that wraps the external [`gitquarry`](https://github.com/Microck/gitquarry) CLI.

It follows the same basic design as [`kagi-mcp`](https://github.com/Microck/kagi-mcp): keep the MCP layer thin, shell out to the real CLI, and return the CLI's results instead of reimplementing product logic in the server.

Built with [rmcp](https://github.com/anthropics/rmcp) (Rust MCP SDK). Uses the `tool_router` / `tool_handler` macros to auto-generate JSON Schema from Rust type definitions, so tool parameters are always in sync with the code.

## Requirements

- **Rust** 2024 edition (1.85+)
- [`gitquarry`](https://github.com/Microck/gitquarry) installed separately and available on `PATH`, or pointed to with `GITQUARRY_CLI_PATH`
- GitHub credentials configured for `gitquarry` ahead of time, either with environment variables or by running `gitquarry auth login` in a real terminal

This server intentionally does not expose `auth login` as an MCP tool. Passing a GitHub personal access token through model tool calls is the wrong default, and the upstream command is TTY-oriented anyway.

## Tools

### `gitquarry_search`

Structured repository search. Maps all search parameters to the underlying `gitquarry search` command with `--format toon --progress off`.

| Parameter | Type | Description |
|-----------|------|-------------|
| `query` | string? | Free-text search query |
| `mode` | enum? | `native` or `discover` |
| `rank` | enum? | `native`, `query`, `activity`, `quality`, or `blended` |
| `sort` | enum? | `best-match`, `stars`, or `updated` |
| `depth` | enum? | `quick`, `balanced`, or `deep` (discover mode) |
| `limit` | u32? | Max repositories to return |
| `user` | string? | Restrict to one user |
| `org` | string? | Restrict to one organization |
| `archived` | enum? | `true` or `false` |
| `template` | enum? | `true` or `false` |
| `fork` | enum? | `false`, `true`, or `only` |
| `language` | string[] | Require one language (AND semantics) |
| `topic` | string[] | Require one topic (AND semantics) |
| `license` | string[] | Require one license (AND semantics) |
| `min_stars` / `max_stars` | u64? | Star count range |
| `min_forks` / `max_forks` | u64? | Fork count range |
| `min_size` / `max_size` | u64? | Repository size range (KB) |
| `created_after` / `created_before` | string? | Date range (YYYY-MM-DD) |
| `updated_after` / `updated_before` | string? | Date range (YYYY-MM-DD) |
| `pushed_after` / `pushed_before` | string? | Date range (YYYY-MM-DD) |
| `created_within` / `updated_within` / `pushed_within` | string? | Recency window (e.g., `30d`, `12h`, `1y`) |
| `readme` | bool? | Include README enrichment |
| `explain` | bool? | Include ranking explanations |
| `weight_query` / `weight_activity` / `weight_quality` | f64? | Blended ranking weights (0.0–3.0) |
| `concurrency` | u32? | Worker count for discover mode |
| `host` | string? | GitHub host override |

### `gitquarry_inspect`

Explicit repository inspection. Returns TOON text for a single `owner/repo`.

| Parameter | Type | Description |
|-----------|------|-------------|
| `repository` | string | Repository identifier in `owner/repo` form (required) |
| `readme` | bool? | Include the repository README |
| `host` | string? | GitHub host override |

### `gitquarry_tree`

Repository tree inspection without cloning. Returns TOON text from `gitquarry tree --format toon --progress off`.

| Parameter | Type | Description |
|-----------|------|-------------|
| `repository` | string | Repository identifier in `owner/repo` form (required) |
| `reference` | string? | Branch, tag, or commit to inspect |
| `paths` | string[] | Glob filters using `*` and `?` |
| `contains` | string? | Keep paths containing this text |
| `depth` | u32? | Maximum path depth to return |
| `host` | string? | GitHub host override |

### `gitquarry_code`

Repository code search without cloning. Returns TOON text from `gitquarry code --format toon --progress off`.

| Parameter | Type | Description |
|-----------|------|-------------|
| `repository` | string | Repository identifier in `owner/repo` form (required) |
| `pattern` | string | Literal text or regex pattern to search for |
| `reference` | string? | Branch, tag, or commit to inspect |
| `paths` | string[] | Candidate file glob filters using `*` and `?` |
| `mode` | enum? | `literal` or `regex` |
| `context` | u32? | Lines before and after each match |
| `limit` | u32? | Maximum matches to return |
| `max_file_bytes` | u64? | Maximum file size to fetch |
| `host` | string? | GitHub host override |

### `gitquarry_auth_status`

Show whether gitquarry has a saved token for the effective host.

### `gitquarry_auth_logout`

Delete the saved gitquarry token for the effective host.

### `gitquarry_config_path`

Print the effective gitquarry config path.

### `gitquarry_config_show`

Print the effective gitquarry config payload as JSON.

### `gitquarry_version`

Print the wrapped gitquarry CLI version.

## MCP Resources and Prompts

The server does not expose custom MCP resources or prompts. It is a pure tool server - all interaction happens through the tools listed above.

## Environment

| Variable | Description | Default |
|----------|-------------|---------|
| `GITQUARRY_CLI_PATH` | Override for the wrapped CLI binary path | `gitquarry` (looked up on `$PATH`) |
| `GITQUARRY_MCP_TIMEOUT_MS` | Command timeout in milliseconds | `30000` |

Any normal `gitquarry` auth env vars still apply, for example `GITQUARRY_TOKEN` or host-specific variants such as `GITQUARRY_TOKEN_GITHUB_COM`.

## Build

```bash
cargo build
```

## Run

```bash
cargo run
```

The server uses stdio transport, so MCP clients should launch the built binary as a command.

## Verify

```bash
cargo test
```

## Error Handling

All tool errors are returned as MCP error results with a descriptive message. Common errors:

- **Spawn failed:** `gitquarry` not found on `$PATH` — set `GITQUARRY_CLI_PATH` or install the CLI
- **Timeout:** command exceeded `GITQUARRY_MCP_TIMEOUT_MS` — increase the value for large discover-mode searches
- **Parse failed:** CLI returned non-JSON output for a JSON-parsed helper — check that gitquarry is up to date
- **Command failed:** CLI exited non-zero — the error message includes stderr output from the underlying command
