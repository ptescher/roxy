# roxy-mcp

MCP (Model Context Protocol) server for Roxy network debugger.

## Overview

This crate exposes Roxy's observability data to AI agents via MCP, enabling them to query HTTP requests, distributed traces, database queries, and Kafka messages captured by the proxy.

## Features

- **stdio transport** - For CLI usage with Claude Desktop and other MCP clients
- **SSE transport** - For remote access, embedded in roxy-proxy by default

## Installation

```sh
cargo install --path crates/roxy-mcp
```

Or build from the workspace root:

```sh
cargo build -p roxy-mcp --release
```

## Usage

### Embedded in roxy-proxy (SSE)

When running roxy-proxy, the MCP server is automatically started on port 3001. Connect to it at:

```
http://localhost:3001/sse
```

Configure in your MCP client using the SSE transport URL.

### With Claude Desktop (stdio)

Add to your Claude Desktop config (`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):

```json
{
  "mcpServers": {
    "roxy": {
      "command": "/path/to/roxy-mcp"
    }
  }
}
```

### Programmatic Usage (Library)

```rust
use roxy_mcp::{run_sse, SseServerConfig};

// Start SSE server on custom port
let config = SseServerConfig {
    bind_address: ([127, 0, 0, 1], 3001).into(),
    ..Default::default()
};
let server = run_sse(config).await?;

// Stop when done
server.stop().await;
```

## Available Tools

| Tool | Description |
|------|-------------|
| `get_recent_http_requests` | Get recent HTTP requests with optional filtering by host, method, status |
| `get_trace_spans` | Get all spans for a specific trace ID |
| `get_recent_spans` | Get recent OpenTelemetry spans |
| `get_request_by_id` | Get full details of a specific HTTP request |
| `get_recent_database_queries` | Get recent database queries (PostgreSQL, etc.) |
| `get_recent_kafka_messages` | Get recent Kafka produce/consume operations |
| `get_host_summary` | Get aggregated statistics by host |
| `search_http_requests` | Search requests by query string |

## Configuration

### Environment Variables

- `RUST_LOG` - Set log level (e.g., `RUST_LOG=roxy_mcp=debug`)

### Cargo Features

- `stdio` - Enable stdio transport (default)
- `sse` - Enable SSE transport for remote/embedded usage

## Requirements

Roxy must be running with ClickHouse to store observability data. The MCP server connects to ClickHouse at `http://localhost:8123` by default.