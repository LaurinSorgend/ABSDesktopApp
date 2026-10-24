# 0003: Realtime client

Status: accepted

## Decision

`abs-api` talks to the Audiobookshelf Socket.IO server with its own small client: Engine.IO v4 and Socket.IO v5 text packets over a WebSocket from `tokio-tungstenite` (rustls). No polling transport, no binary packets, default namespace only.

## Reasons

- `rust_socketio` 0.6.0, the crate the plan named, pulls in `native-tls` (OpenSSL on Linux), tungstenite 0.21 and a second hyper/TLS stack next to reqwest's.
- The client needs few packet types: open, ping/pong, namespace connect, events, close. Both test servers (socket.io 4.7.4) accept a direct WebSocket connection.
- With our own code, re-auth can follow token changes directly (`AbsClient::token_changes`) instead of a timer.

## Consequences

- The `Realtime` type in `abs-api` is the boundary the plan wanted a trait for; it can be reimplemented without touching `core`.
- If a server sits behind a proxy that blocks WebSocket upgrades, there is no long-polling fallback. Add one only if users report it.
- The socket path is `<base path>/socket.io/`; ABS serves it there when `ROUTER_BASE_PATH` is set, and at `/socket.io/` otherwise.
