# Sujiro Kimiskute

<img src="https://i.imgur.com/2xA2jME.png" alt="Sujiro Kimiskute" />

A fast, clean, and lightweight Discord music bot written in Rust. Plays YouTube audio via slash commands. No bloat, no dashboards, no databases — just music.

Runs comfortably on a Raspberry Pi, a cheap VPS, or even an Android phone with [UserLAnd](https://userland.tech/) or [Termux](https://termux.dev/).

## Why Sujiro?

Most people want a Discord bot that plays music. That's it. No web panels, no lyrics, no voting systems — just drop in a link and listen. Sujiro does exactly that and stays out of your way.

## About the name

**Sujiro Kimiskute** is a Japanese-sounding phonetic pun on the Portuguese phrase *"Sugiro que me escute"* — "I suggest you listen to me." A fitting name for a music bot.

## Quick start

### Dependencies

```bash
# Ubuntu / Debian
sudo apt install -y build-essential pkg-config libopus-dev ffmpeg pipx
pipx ensurepath && pipx install yt-dlp

# Rust (1.88+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Discord setup

1. Create an app at [Discord Developer Portal](https://discord.com/developers/applications).
2. Add a bot, copy the token and Application ID.
3. In OAuth2 > URL Generator, pick `bot` + `applications.commands` scopes.
4. Grant: View Channel, Send Messages, Connect, Speak.
5. Invite the bot with the generated URL.

### Configure

```bash
cp .env.example .env
# Fill in DISCORD_TOKEN and DISCORD_APPLICATION_ID
```

See `.env.example` for optional settings (timeouts, queue size, auto-leave, etc.).

`BOT_LANGUAGE` controls the bot-wide language for command descriptions, responses, embeds, and controls. Supported values are `pt-BR` and `en-US`; when the variable is omitted, the bot defaults to `pt-BR`. Restart the bot after changing the language. Slash command names remain in English in both languages.

### Run

```bash
cargo build --release
./target/release/sujiro-kimiskute
```

Or with Docker:

```bash
docker build -t sujiro-kimiskute:local .
docker run --rm --env-file .env sujiro-kimiskute:local
```

## Commands

| Command    | Description                                          |
| ---------- | ---------------------------------------------------- |
| `/play`    | Search, video URL, or playlist URL from YouTube      |
| `/pause`   | Pause the current track                              |
| `/resume`  | Resume playback                                     |
| `/skip`    | Skip to the next track                               |
| `/stop`    | Stop playback and clear the queue                    |
| `/queue`   | Show current track and next up to 10                 |
| `/leave`   | Clear queue, disconnect, and remove guild state       |

`/play` requires you to be in a voice channel. Control commands (`/pause`, `/resume`, `/skip`, `/stop`) require you to be in the same channel as the bot. One session per server.

The bot auto-disconnects after `AUTO_LEAVE_SECONDS` alone in the channel.

## Architecture

```
discord/   — Serenity handlers, slash commands, UI embeds
player/    — Queue, playback state, guild lifecycle, auto-leave
sources/   — Source resolution (currently YouTube via yt-dlp)
voice/     — Songbird voice connection and event handling
config/    — Environment-based configuration
state/     — Shared application state
```

YouTube-specific logic stays in `sources/youtube/`. The resolver trait is designed so Spotify or other sources can be added later without touching command handlers.

## YouTube PO Tokens

A Proof of Origin (PO) Token lets YouTube verify that a request came from a genuine client. YouTube is gradually enforcing these tokens; without one, yt-dlp may expose fewer formats, receive HTTP 403 responses, or have the account/IP temporarily blocked.

Sujiro only invokes yt-dlp: it does not generate or store PO Tokens. The recommended setup is a [PO Token Provider plugin](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide), installed on the same host as yt-dlp (or inside the same container). Once the provider and its dependencies are configured, select the currently recommended `mweb` client in `.env`:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb
```

Manual setup (**advanced, not recommended**) uses the current extractor argument format:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb;po_token=mweb.gvs+TOKEN
```

Do not commit or log PO Tokens or YouTube cookies. Keep them in `.env` and rotate them immediately if exposed. Manual tokens may be bound to a session or video and have a limited lifetime, which is why a provider is preferred. Docker users must build a custom image containing the provider plugin and all of its runtime dependencies; configuring the host alone is not enough.

See the official yt-dlp [PO Token Guide](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) and [YouTube extractor notes](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#youtube) for current requirements.

## Limitations (by design)

- YouTube only (Spotify planned)
- Slash commands only
- In-memory state — lost on restart
- No database, web dashboard, autoplay, filters, or lyrics
- Playlist length capped by `MAX_QUEUE_SIZE`

## Validate

```bash
cargo fmt --check
cargo check
cargo clippy -- -D warnings
cargo build --release
```
