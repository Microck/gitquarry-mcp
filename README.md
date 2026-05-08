# gitquarry-mcp

`gitquarry-mcp` is a small stdio MCP server that wraps the external [`gitquarry`](https://github.com/Microck/gitquarry) CLI.

It follows the same basic design as [`kagi-mcp`](https://github.com/Microck/kagi-mcp): keep the MCP layer thin, shell out to the real CLI, and return the CLI's results instead of reimplementing product logic in the server.

## Requirements

- Rust toolchain for building this server
- `gitquarry` installed separately and available on `PATH`, or pointed to with `GITQUARRY_CLI_PATH`
- GitHub credentials configured for `gitquarry` ahead of time, either with environment variables or by running `gitquarry auth login` in a real terminal

This server intentionally does not expose `auth login` as an MCP tool. Passing a GitHub personal access token through model tool calls is the wrong default, and the upstream command is TTY-oriented anyway.

## Tools

- `gitquarry_search` - structured repository search via `gitquarry search --format json --progress off`
- `gitquarry_inspect` - explicit repository inspection via `gitquarry inspect --format json --progress off`
- `gitquarry_tree` - repository tree inspection via `gitquarry tree --format json --progress off`
- `gitquarry_code` - repository code search via `gitquarry code --format json --progress off`
- `gitquarry_auth_status` - text status for the effective host
- `gitquarry_auth_logout` - remove the saved token for the effective host
- `gitquarry_config_path` - print the config path
- `gitquarry_config_show` - return the effective config payload as JSON
- `gitquarry_version` - print the wrapped CLI version

## Environment

- `GITQUARRY_CLI_PATH` - optional override for the wrapped CLI binary path
- `GITQUARRY_MCP_TIMEOUT_MS` - optional command timeout in milliseconds, default `30000`

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
