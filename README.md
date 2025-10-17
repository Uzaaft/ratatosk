# Ratatosk

A simple TUI application for visualizing tracing logs in real-time.

## Features

- **Trace View**: Display all incoming trace logs
- **Latency Bar Chart**: Show average API route latencies
- **Error/Warning Panel**: Highlight errors and warnings

## Usage

Pipe trace logs into the application:

```bash
cargo build --release
your-app | ./target/release/ratatosk
```

Or for testing:
```bash
echo '{"route":"/api/users","latency":150}' | cargo run
```

Press `q` to quit.

## Log Format

Supports both JSON and text formats:

**JSON:**
```json
{"route":"/api/users","latency":150}
```

**Text:**
```
route=/api/users latency=150
ERROR something went wrong
WARN potential issue
```
