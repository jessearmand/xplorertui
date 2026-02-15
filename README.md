# xplorertui

A terminal UI for browsing X (Twitter), built with [Ratatui] and the X API v2.

Browse your home timeline, mentions, bookmarks, search tweets, view threads, and look up user profiles — all from your terminal. Based on the Ratatui [event driven async template].

[Ratatui]: https://ratatui.rs
[event driven async template]: https://github.com/ratatui/templates/tree/main/event-driven-async

## Getting Started

### Prerequisites

- Rust (2024 edition) — install via [rustup](https://rustup.rs)
- An [X Developer account](https://developer.x.com) with API credentials

### Build & Run

```bash
cargo build --release
cargo run
```

Enable debug logging by setting `RUST_LOG`:

```bash
RUST_LOG=debug cargo run 2>debug.log
```

### Configuration

Optional configuration file at `~/.config/xplorertui/config.toml`:

```toml
tick_rate_fps = 30       # UI refresh rate
default_max_results = 20 # Tweets per API request (10–100)
default_view = "home"    # One of: home, mentions, bookmarks, search
```

## Authentication

xplorertui supports three auth methods, auto-detected from environment variables. Place them in a `.env` file at one of these locations (highest priority first):

1. `~/.config/xplorertui/.env`
2. `~/.config/x-cli/.env`
3. `./.env` (current directory)

### OAuth 2.0 PKCE (recommended)

Enables full user-context access (home timeline, mentions, bookmarks). On first run, a browser window opens for authorization. Tokens are persisted at `~/.config/xplorertui/tokens.json`.

```env
X_CLIENT_ID=your_client_id
X_CLIENT_SECRET=your_client_secret  # optional for public clients
```

### OAuth 1.0a

Full user-context access using HMAC-SHA1 signed requests.

```env
X_API_KEY=your_api_key
X_API_SECRET=your_api_secret
X_ACCESS_TOKEN=your_access_token
X_ACCESS_TOKEN_SECRET=your_access_token_secret
X_BEARER_TOKEN=your_bearer_token  # optional, used for read-only endpoints
```

### App-only Bearer Token

Read-only access. User-context endpoints (home timeline, mentions, bookmarks) will not be available.

```env
X_BEARER_TOKEN=your_bearer_token
```

## Keybindings

### Navigation

| Key | Action |
|---|---|
| `j` / `Down` | Move down |
| `k` / `Up` | Move up |
| `Enter` | Open selected item (thread view) |
| `Esc` / `q` | Go back / quit |
| `n` | Load next page |

### Views

| Key | Action |
|---|---|
| `1` | Home timeline |
| `2` | Mentions |
| `3` | Bookmarks |
| `4` | Search |
| `?` | Help overlay |

### Input Modes

| Key | Action |
|---|---|
| `:` | Command mode |
| `/` | Search tweets |
| `@` | Look up user |
| `Ctrl-C` | Quit |

### Commands

Type `:` to enter command mode, then:

| Command | Action |
|---|---|
| `:user <username>` | View a user's profile |
| `:search <query>` | Search tweets |
| `:open <url or id>` | Open a tweet by URL or ID |
| `:home` | Switch to home timeline |
| `:mentions` / `:m` | Switch to mentions |
| `:bookmarks` / `:b` | Switch to bookmarks |
| `:help` / `:h` | Show help |
| `:quit` / `:q` | Quit |

## License

Copyright (c) Jesse Armand <jesse@jessearmand.com>

This project is licensed under the MIT license ([LICENSE] or <http://opensource.org/licenses/MIT>)

[LICENSE]: ./LICENSE
