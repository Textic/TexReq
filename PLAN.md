# TexReq — Architecture & Implementation Plan

A fast, native desktop API testing client built in Rust with **GPUI-Kit**. Designed with a clean, flat dark interface (solid backgrounds, zero gradients, crisp `1px` panel cuts) inspired by modern minimalist desktop tools.

---

## Core Capabilities

1. **Multi-Protocol Request Engine (`src/net/`)**
   - **HTTP / HTTPS**: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS` with `reqwest` + `rustls`.
   - **WebSockets (`ws://` / `wss://`)**: Live bidirectional connection, message templates, searchable live message log, and TLS certificate toggle.
   - **SSE (Server-Sent Events)**: Streaming endpoint listener with live event frame log.
   - **GraphQL**: Dedicated query + JSON variables editor.
   - **Request Bodies**: JSON (with pretty formatter), Raw Text/XML/HTML/JS, URL-encoded Form, Multipart Form-Data (with file picker), and Binary file upload.
   - **Auth**: None, Bearer Token, Basic Auth, and API Key (header or query parameter).
   - **Response Captures**: Extract JSON dot-path values (`$.data.token`) directly into active environment variables after each response.
   - **Network Controls**: Per-request or global timeout, HTTP redirects toggle, SSL/TLS verification toggle, and custom HTTP/HTTPS proxy.

2. **Flat Dark UI Architecture (`src/ui/`, `src/theme.rs`)**
   - **Frameless Window**: Integrated draggable top header bar alongside native-style window minimize/maximize/close buttons.
   - **Left Sidebar (`sidebar.rs`)**: Circular brand mark, New Request (`+`), New Folder (`Folder`), Settings gear (`Settings`), search filter, Collections tree, and History list.
   - **Main Split Workspace (`workspace.rs`, `request_pane.rs`, `response_pane.rs`)**: Toggleable vertical or side-by-side layout between Request composer and Response / Stream inspector.
   - **Right Rail & Drawers (`right_drawer.rs`)**:
     - Code Snippets (`cURL`, `fetch`, `Rust reqwest`, `Python requests`).
     - Environment Variables editor + `.env` importer.
     - Network & TLS timing inspector (`TTFB`, download duration, sent headers).
     - **Settings Drawer**: Auto-save toggle, JSON auto-format, global SSL/TLS verification & redirect defaults, default timeout/proxy, workspace folder picker, and integrated Git workflow (`init`, `commit`, `pull`, `push`).

3. **Workspace Storage & Git Version Control (`src/storage/`)**
   - Collections (`collections.json`) and environments (`environments.toml`) live inside a configurable local **Workspace Folder** (defaulting to the user's `TexReq` config directory) so teams or individuals can track them directly with Git.
   - Transient local state (`history.json`) and machine preferences (`settings.json`) are stored separately in the user config directory and excluded from workspace Git commits via `.gitignore`.
   - Note for Windows debug builds: `build.rs` sets `/STACK:8388608` so deep GPUI layout trees run without overflowing the default 1 MB Windows debug stack.
