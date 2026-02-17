# xplorertui

A terminal UI and CLI for browsing X, built with [Ratatui] and the X API v2.

Browse your home timeline, mentions, bookmarks, search tweets, view threads, and look up user profiles — all from your terminal. Includes a non-interactive CLI mode that outputs JSONL to stdout for piping into tools like `jq`, `grep`, and `wc`. Based on the Ratatui [event driven async template]. Several implementations — including the X API v2 client, authentication strategies, and credential handling — were adapted from [x-cli], a command-line client for X.

[Ratatui]: https://ratatui.rs
[event driven async template]: https://github.com/ratatui/templates/tree/main/event-driven-async
[x-cli]: https://github.com/Infatoshi/x-cli

## Getting Started

### Prerequisites

- Rust (2024 edition) — install via [rustup](https://rustup.rs)
- An [X Developer account](https://developer.x.com) with API credentials

### Install

```bash
cargo install --path .
```

Or build and run without installing:

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

Enables full user-context access (home timeline, mentions, bookmarks). Tokens are persisted at `~/.config/xplorertui/tokens.json` and automatically refreshed when they expire.

```env
X_CLIENT_ID=your_client_id
X_CLIENT_SECRET=your_client_secret  # optional for public clients
```

After setting up your `.env` file, authenticate before launching the TUI:

```bash
xplorertui auth
```

This opens your browser for authorization and saves the tokens. You can also authenticate from within the TUI by typing `:auth` in command mode.

### OAuth 1.0a

Full user-context access using HMAC-SHA1 signed requests.

```env
X_CONSUMER_KEY=your_consumer_key
X_CONSUMER_KEY_SECRET=your_consumer_key_secret
X_ACCESS_TOKEN=your_access_token
X_ACCESS_TOKEN_SECRET=your_access_token_secret
X_BEARER_TOKEN=your_bearer_token  # optional, used for read-only endpoints
```

### App-only Bearer Token

Read-only access. User-context endpoints (home timeline, mentions, bookmarks) will not be available.

```env
X_BEARER_TOKEN=your_bearer_token
```

## CLI Mode

When a subcommand is provided, xplorertui bypasses the TUI and outputs JSONL (one JSON object per line) to stdout. This makes it easy to pipe X API data into other tools.

```bash
xplorertui                          # Launch TUI (default)
xplorertui tui                      # Launch TUI (explicit)
xplorertui auth                     # OAuth 2.0 PKCE flow
xplorertui home                     # Home timeline → JSONL
xplorertui mentions                 # Mentions → JSONL
xplorertui bookmarks                # Bookmarks → JSONL
xplorertui search <query>           # Search tweets → JSONL
xplorertui user <username>          # User profile → JSONL
xplorertui open <tweet_id_or_url>   # Single tweet + thread → JSONL
```

Each tweet line is a denormalized JSON object with the tweet, its author, and any attached media embedded:

```bash
# Pretty-print your home timeline
xplorertui home | jq .

# Count mentions
xplorertui mentions | wc -l

# Search and filter with jq
xplorertui search "rust lang" | jq '.tweet.text'

# Open a tweet by URL
xplorertui open https://x.com/user/status/1234567890
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
| `:auth` / `:login` | Authenticate with OAuth 2.0 PKCE |
| `:help` / `:h` | Show help |
| `:quit` / `:q` | Quit |

## License

Copyright (c) Jesse Armand <jesse@jessearmand.com>

This project is licensed under the MIT license ([LICENSE] or <http://opensource.org/licenses/MIT>)

[LICENSE]: ./LICENSE
