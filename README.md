# Sujiro Kimiskute

<img src="https://i.imgur.com/2xA2jME.png" alt="Sujiro Kimiskute" />

A fast, lightweight Discord music bot written in Rust. It plays YouTube audio through slash commands. No dashboards, no databases, no bloat. Just music.

It is light enough to run on a Raspberry Pi, a cheap VPS, or even an Android phone through [UserLAnd](https://userland.tech/) or [Termux](https://termux.dev/).

**Other languages:** [Português](README.pt-BR.md) · [Español](README.es.md) · [Français](README.fr.md) · [Italiano](README.it.md)

## Install in one command

On any 64-bit Linux system this is all you need. The installer detects your CPU (x86_64 or ARM64), downloads and verifies the matching release, installs `yt-dlp` if it is missing (the official zipapp when Python 3.10+ is available, the standalone binary otherwise), sizes the yt-dlp settings for your host, and walks you through creating the `.env` file:

```bash
curl -fsSLO https://raw.githubusercontent.com/lexmarcos/sujiro-kimiskute/main/install.sh
chmod +x install.sh
./install.sh
```

That is it. The bot lands in `~/.local/share/sujiro-kimiskute` and a launcher goes into `~/.local/bin`. Want different paths? Set `SUJIRO_VERSION`, `SUJIRO_INSTALL_DIR`, or `SUJIRO_BIN_DIR` before running the script.

You will need a Discord token first, so keep the next section handy while the installer runs.

## Discord setup

1. Create an app at the [Discord Developer Portal](https://discord.com/developers/applications).
2. Add a bot, then copy the token and the Application ID.
3. In OAuth2 > URL Generator, pick the `bot` and `applications.commands` scopes.
4. Grant View Channel, Send Messages, Connect, and Speak.
5. Invite the bot with the generated URL.

## Why Sujiro?

Most people just want a bot that plays music. No web panels, no lyrics, no voting systems. Drop in a link and listen. Sujiro does exactly that and stays out of your way.

The name is a Japanese-sounding pun on the Portuguese phrase *"Sugiro que me escute"*, meaning "I suggest you listen to me." Fitting for a music bot.

## Commands

| Command   | What it does                                             |
| --------- | ------------------------------------------------------- |
| `/play`   | Search, YouTube/YouTube Music video, Short, or playlist |
| `/pause`  | Pause the current track                                 |
| `/resume` | Resume playback                                         |
| `/skip`   | Skip to the next track                                  |
| `/stop`   | Stop playback and clear the queue                       |
| `/queue`  | Show the current track and up to 10 coming next         |
| `/leave`  | Clear the queue, disconnect, and drop the server state  |

`/play` requires you to be in a voice channel. The control commands (`/pause`, `/resume`, `/skip`, `/stop`) require you to be in the same channel as the bot. One session per server. The bot leaves on its own after `AUTO_LEAVE_SECONDS` when left alone in the channel.

## Build from source

Prefer to compile it yourself? Clone the repository and build the release binary.

Install the dependencies (Ubuntu / Debian):

```bash
sudo apt install -y build-essential pkg-config libopus-dev ffmpeg pipx
pipx ensurepath && pipx install 'yt-dlp[default]'
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust 1.88+
```

Then clone and build:

```bash
git clone https://github.com/lexmarcos/sujiro-kimiskute.git
cd sujiro-kimiskute
cp .env.example .env          # fill in DISCORD_TOKEN and DISCORD_APPLICATION_ID
cargo build --release
./target/release/sujiro-kimiskute
```

Or run it with Docker:

```bash
docker build -t sujiro-kimiskute:local .
docker run --rm --env-file .env sujiro-kimiskute:local
```

## Configuration

Every setting lives in `.env`. Copy `.env.example` to get started, then fill in `DISCORD_TOKEN` and `DISCORD_APPLICATION_ID`. Everything else is optional (timeouts, queue size, auto-leave, and more).

Notable settings:

- `BOT_LANGUAGE` sets the language for command descriptions, responses, embeds, and controls. Supported values are `pt-BR` and `en-US`, defaulting to `pt-BR` when omitted. Slash command names stay in English either way.
- `BOT_ACTIVITY_TYPE` and `BOT_ACTIVITY_MESSAGE` set the presence shown on the bot. The type is case-sensitive and accepts `playing`, `watching`, `listening`, or `competing`. They default to `listening` and `música`.
- `BOT_ACTIVITY_CURRENT_TRACK=true` shows the current title when exactly one server is actively playing. With zero or multiple active servers, the bot uses the configured static activity.
- `PLAYER_PANEL_UPDATE_SECONDS` controls live playback-progress updates. It defaults to `5`; positive values set the interval in seconds. Set it to `0` to disable the progress display and periodic panel edits while keeping immediate updates for pause, skip, stop, and track changes.
- `IDLE_LEAVE_SECONDS` disconnects after the queue finishes even if listeners remain. It defaults to 300 seconds; set it to `0` to disable it.
- `YT_DLP_EXTRA_ARGS` passes extra flags to yt-dlp (shell-style quoting). The bot always adds `skip=hls,dash` to your `youtube:` extractor arguments because it can only play plain HTTP streams. Keep every YouTube setting in a single `--extractor-args youtube:...` value: yt-dlp only keeps the last one per extractor, and the one the bot passes also overrides a `youtube:` section in a yt-dlp config file.
- `YT_DLP_TIMEOUT_SECONDS` (default `45`) bounds each yt-dlp run. YouTube's JS challenge takes about two CPU seconds on a desktop and ten times that on a low-end ARM board, so use `60` there.
- `MAX_CONCURRENT_RESOLUTIONS` (default `2`) caps parallel yt-dlp runs. Each can peak near 300 MB when a JavaScript runtime is involved; use `1` on hosts with one vCPU or 1 GB of RAM.

Restart the bot after changing any of these.

### Playback behavior

- YouTube, YouTube Music, `youtu.be`, and Shorts links are accepted.
- Shared `t=` or `start=` timestamps begin playback at that position.
- A YouTube URL containing a non-empty `list=` parameter queues the playlist, including watch URLs that also select a video with `v=`.
- Playlist requests show loading feedback and can be canceled before they are committed to the queue.
- The final response reports unavailable playlist entries and tracks omitted by the queue limit.
- The latest `/play` or `/queue` response becomes the single live player panel, shows current and queue timing estimates, and updates as playback changes.
- If a stream fails, the bot refreshes it once. If recovery also fails, the unavailable track is skipped and the channel is notified.
- While a track plays, the stream of the next queued track is prepared in the background, so the transition does not wait for yt-dlp. The prefetch only runs when a resolution slot is free.

## Low-power hosts

Sujiro itself is cheap: YouTube's Opus audio is passed straight through to Discord without decoding. Almost all CPU, memory, and waiting go to `yt-dlp`, and three choices decide how much:

1. **JavaScript runtime.** Recent yt-dlp solves YouTube's player challenge with Deno (or Node via `--js-runtimes node`). Measured on a desktop, one resolution costs about 0.6 CPU seconds and 60 MB without a runtime versus 2 CPU seconds, 300 MB, and one more request with one; low-end ARM boards are roughly ten times slower. Without a runtime yt-dlp falls back to a single client that needs no challenge. That works today, but yt-dlp marks it deprecated and YouTube changes which clients work without PO tokens. On a weak host, start without a runtime and add Deno (or `--js-runtimes node` in `YT_DLP_EXTRA_ARGS`) only when playback breaks.
2. **Requests per track.** The bot already skips the HLS and DASH manifests. `player_skip=webpage,configs` also skips the 1.4 MB watch page and shrinks yt-dlp's output from about 600 KB to 100 KB of JSON:

   ```dotenv
   YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_skip=webpage,configs
   YT_DLP_TIMEOUT_SECONDS=60
   MAX_CONCURRENT_RESOLUTIONS=1
   ```

   The installer applies this profile on hosts with up to 2 CPUs or 2 GB of RAM. Drop `player_skip` if you use PO tokens with web clients: they need the watch page.
3. **How yt-dlp is installed.** `pip install 'yt-dlp[default]'` (or pipx) starts in about 0.15 s on a desktop, the zipapp in 0.5 s, and the standalone `yt-dlp_linux` binary in 0.6 s while unpacking 40 MB into `/tmp` on every run, which hurts on slow storage and under proot (UserLAnd). The installer prefers the zipapp whenever Python 3.10+ is present. Keep yt-dlp updated (`yt-dlp -U`) and keep `~/.cache/yt-dlp` writable and persistent: it stores solved player signatures.

Measure on your own device with `RUST_LOG=info`: every resolution logs `yt-dlp process finished` with its `duration_ms`.

## YouTube PO tokens

A Proof of Origin (PO) token lets YouTube verify that a request came from a genuine client. YouTube is gradually enforcing these; without one, yt-dlp may expose fewer formats, hit HTTP 403 responses, or get the account or IP temporarily blocked.

Sujiro only invokes yt-dlp. It does not generate or store PO tokens. The recommended setup is a [PO Token Provider plugin](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) installed on the same host as yt-dlp (or inside the same container). Once the provider is ready, select the recommended `mweb` client in `.env`:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb
```

Manual setup is possible but advanced and not recommended:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb;po_token=mweb.gvs+TOKEN
```

Keep every `youtube:` setting in that single value (yt-dlp keeps only the last `--extractor-args` per extractor; the bot merges its own `skip=hls,dash` into yours), and do not combine it with `player_skip=webpage`: PO tokens for web clients need the watch page.

Never commit or log PO tokens or YouTube cookies. Keep them in `.env` and rotate them right away if they leak. Manual tokens can be bound to a session or a single video and expire quickly, which is why a provider is preferred. Docker users must build a custom image that bundles the provider plugin and all of its runtime dependencies, since configuring the host alone is not enough.

See the official yt-dlp [PO Token Guide](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) and [YouTube extractor notes](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#youtube) for current requirements.

## Architecture

```
discord/   Serenity handlers, slash commands, UI embeds
player/    queue, playback state, guild lifecycle, auto-leave
sources/   source resolution (currently YouTube via yt-dlp)
voice/     Songbird voice connection and event handling
config/    environment-based configuration
state/     shared application state
```

YouTube-specific logic stays in `sources/youtube/`. The resolver trait is designed so Spotify or other sources can be added later without touching the command handlers.

## Limitations (by design)

- YouTube only (Spotify is planned)
- Slash commands only
- In-memory state, lost on restart
- No database, web dashboard, autoplay, filters, or lyrics
- Playlist length capped by `MAX_QUEUE_SIZE`
