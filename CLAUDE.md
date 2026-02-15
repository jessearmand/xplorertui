# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**xplorertui** is an event-driven async terminal UI for browsing the X (Twitter) platform. Built with Ratatui + Crossterm + Tokio, targeting the X API v2. Rust 2024 edition.

## Build & Development Commands

```bash
cargo build                          # Debug build
cargo build --release                # Release build (LTO, stripped)
cargo run                            # Run the app (needs credentials)
cargo test --locked --all-features --all-targets  # Run all tests
cargo test --test <name>             # Run a specific integration test
cargo test command::tests            # Run module-specific unit tests
cargo fmt -- --check                 # Check formatting
cargo clippy                         # Lint
cargo doc --no-deps --all-features   # Generate docs
RUST_LOG=debug cargo run 2>debug.log # Run with tracing output to file
```

## Architecture

### Event Loop (Unidirectional Data Flow)

The app follows a **single-threaded event loop** with async API dispatch:

1. **`EventHandler`** (`src/event.rs`) — spawns a background task that merges tick events (30 FPS) and crossterm terminal events into a single `mpsc` channel. App-level events (`AppEvent`) are sent through the same channel.

2. **`App::run()`** (`src/app.rs`) — the main loop: draw → receive event → handle. Key presses produce `AppEvent`s (e.g., `FetchHomeTimeline`). API request events are dispatched to `tokio::spawn` tasks.

3. **`dispatch_api_request()`** — clones an `Arc<Mutex<XApiClient>>` into a spawned task, makes the API call, then sends a `*Loaded` response event back through the channel.

4. **`handle_app_event()`** — processes response events, updates `App` state (timelines, users, threads), and sets status messages on errors.

### AppEvent Enum

`AppEvent` in `src/event.rs` serves dual purpose:
- **Request variants** (`Fetch*`) — triggers dispatched from key handlers
- **Response variants** (`*Loaded`) — results from async API tasks carrying `ApiResult<T>`

`ApiResult<T>` uses `Arc<String>` for errors to satisfy `Clone`.

### View Stack Navigation

`App.view_stack: Vec<ViewState>` acts as a navigation stack. `ViewKind` identifies the current view (Home, Mentions, Bookmarks, Search, UserProfile, UserTimeline, Thread, Help). Push/pop for drill-down and back navigation; `SwitchView` replaces the root.

### Module Layout

- **`src/app.rs`** — `App` struct (all state), key handling, event loop, API dispatch
- **`src/event.rs`** — `Event`, `AppEvent`, `ViewKind`, `EventHandler`
- **`src/command.rs`** — `:command` parser (vim-style commands like `:user`, `:search`, `:quit`)
- **`src/config.rs`** — TOML config from `~/.config/xplorertui/config.toml`
- **`src/ui/`** — Ratatui widget modules. `ui::draw()` in `mod.rs` dispatches to per-view widgets. Each view (timeline, tweet, thread, user, search, bookmarks, help, status_bar, command_bar) is a separate widget module.
- **`src/api/`** — X API v2 client. `mod.rs` has `XApiClient` with `bearer_get`/`oauth_get` methods. Endpoint methods split across `tweets.rs`, `users.rs`, `engagement.rs`. `types.rs` defines all API response types.
- **`src/auth/`** — Auth strategies: OAuth 2.0 PKCE (`oauth2_pkce.rs`), OAuth 1.0a HMAC-SHA1 (`oauth1.rs`), bearer-only. `credentials.rs` loads from `.env` files. Auth method auto-detected by priority: OAuth2 PKCE > OAuth1 > Bearer.

### Authentication & Credentials

Credentials loaded from environment variables via `.env` files in priority order:
1. `~/.config/xplorertui/.env`
2. `~/.config/x-cli/.env`
3. `./.env` (cwd)

| Variable | Auth Method |
|---|---|
| `X_CONSUMER_KEY`, `X_CONSUMER_KEY_SECRET`, `X_ACCESS_TOKEN`, `X_ACCESS_TOKEN_SECRET` | OAuth 1.0a |
| `X_CLIENT_ID`, `X_CLIENT_SECRET` (optional) | OAuth 2.0 PKCE |
| `X_BEARER_TOKEN` | App-only bearer |

OAuth2 PKCE tokens are persisted at `~/.config/xplorertui/tokens.json`.

User-context endpoints (home timeline, mentions, bookmarks) use `oauth_get`; read-only endpoints use `bearer_get`.

### Users Cache

`App.users_cache: HashMap<String, User>` caches user objects from API `includes` fields. Used by UI widgets for author lookup on tweets via `app.lookup_user(author_id)`.

## CI

GitHub Actions runs: `fmt`, `clippy`, `doc` (nightly), and `test` on macOS + Windows.
