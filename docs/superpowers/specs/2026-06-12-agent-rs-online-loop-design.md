# Agent RS Online Loop Design

## Goal

Build the first useful Rust agent milestone: it can authenticate with the existing kelicloud backend, upload basic host information, keep a report WebSocket open, send JSON reports compatible with `common.Report`, and parse backend control messages.

## Scope

This milestone is intentionally smaller than a full Go agent replacement. It includes configuration parity for the core connection options, basic info upload request construction, report payload modeling, report WebSocket client loop, and typed parsing for backend messages: `cn_connectivity_probe_config`, `terminal`, `exec`, and `ping`.

This milestone does not implement real terminal sessions, remote command execution, auto update, auto discovery, GPU probing, or full OS-specific metric collection. Those should be layered on after the network protocol is proven.

## Architecture

The Rust agent stays split into focused modules:

- `config`: command-line and environment configuration.
- `protocol`: URL construction and backend message parsing.
- `report`: JSON models matching backend `common.Report` and static basic info payloads.
- `transport`: HTTP and WebSocket client interfaces.
- `runtime`: orchestration for basic info upload, WebSocket connection, report sending, ping heartbeat, and inbound message dispatch.

The runtime depends on traits for transport and report generation so most behavior can be tested without opening network sockets. The concrete HTTP/WebSocket implementation is thin and can be swapped later if we need custom DNS or proxy behavior matching the Go agent.

## Data Flow

1. Parse config from args/env.
2. Build `/api/clients/uploadBasicInfo?token=...` and `/api/clients/report?token=...`.
3. Upload static basic info once on startup.
4. Connect the report WebSocket.
5. Send one report immediately, then continue at the configured interval.
6. Send WebSocket ping heartbeats.
7. Parse backend text messages into typed control messages and pass them to a handler.

## Error Handling

Configuration and URL errors fail fast with clear messages. HTTP non-success responses are surfaced as transport errors. WebSocket send/read failures end the current connection attempt so reconnect policy can be added cleanly. Token rotation from auto-discovery is not included in this milestone.

## Testing

Tests cover config parsing, URL construction, JSON report serialization, backend control-message parsing, and runtime orchestration using fake transports. The real network client only gets lightweight construction tests in this milestone.
