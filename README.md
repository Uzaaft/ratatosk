# Ratatosk

A composable TUI application for visualizing tracing logs in real-time.

## Features

- **Trace View**: Display all incoming trace logs with scrolling
- **Latency Bar Chart**: Show average API route latencies
- **Error/Warning Panel**: Highlight errors and warnings
- **Configurable Fields**: Define custom field names for route and latency
- **Nested JSON Support**: Extract values from nested JSON objects

## Usage

### Basic Usage

```bash
cargo build --release
your-app | ./target/release/ratatosk
```

### Configurable Fields

**Default (route and latency):**
```bash
echo '{"route":"/api/users","latency":150}' | cargo run
```

**Custom field names via CLI:**
```bash
your-app | ratatosk --route-field=http.endpoint --latency-field=metrics.duration_ms
```

**Environment variables:**
```bash
export RAT_ROUTE_FIELD=api.path
export RAT_LATENCY_FIELD=timing.latency
your-app | ratatosk
```

**Latency scaling (e.g., microseconds to milliseconds):**
```bash
your-app | ratatosk --latency-scale=1000
```

**Config file:**
```bash
ratatosk --config=config.json < logs.txt
```

config.json:
```json
{
  "route_field": "http.route",
  "latency_field": "metrics.latency_us",
  "latency_scale": 1000
}
```

### Navigation

- **Tab**: Switch between Traces and Errors panels
- **↑/↓ or j/k**: Scroll up/down
- **←/→ or h/l**: Horizontal scroll
- **PgUp/PgDn**: Page up/down
- **Home/End**: Jump to start/end
- **q**: Quit

## Log Format

Supports both JSON and text formats:

**JSON (default fields):**
```json
{"route":"/api/users","latency":150}
```

**JSON (nested fields):**
```json
{"http":{"route":"/api/users"},"metrics":{"latency_ms":150}}
```

**Text:**
```
route=/api/users latency=150
ERROR something went wrong
WARN potential issue
```

## Configuration Priority

1. CLI flags (`--route-field`, `--latency-field`, `--latency-scale`)
2. Config file (`--config`)
3. Environment variables (`RAT_ROUTE_FIELD`, `RAT_LATENCY_FIELD`, `RAT_LATENCY_SCALE`)
4. Defaults (`route`, `latency`, scale `1`)
