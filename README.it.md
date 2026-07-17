# Sujiro Kimiskute

<img src="https://i.imgur.com/2xA2jME.png" alt="Sujiro Kimiskute" />

Un bot musicale per Discord veloce e leggero, scritto in Rust. Riproduce l'audio di YouTube tramite comandi slash. Niente dashboard, niente database, niente peso inutile. Solo musica.

È abbastanza leggero da girare su un Raspberry Pi, su un VPS economico o persino su un telefono Android con [UserLAnd](https://userland.tech/) o [Termux](https://termux.dev/).

**Altre lingue:** [English](README.md) · [Português](README.pt-BR.md) · [Español](README.es.md) · [Français](README.fr.md)

## Installa con un solo comando

Su qualsiasi sistema Linux a 64 bit è tutto ciò che ti serve. L'installer rileva il tuo processore (x86_64 o ARM64), scarica e verifica la release corrispondente, installa `yt-dlp` se manca e ti guida nella creazione del file `.env`:

```bash
curl -fsSLO https://raw.githubusercontent.com/lexmarcos/sujiro-kimiskute/main/install.sh
chmod +x install.sh
./install.sh
```

Fatto. Il bot finisce in `~/.local/share/sujiro-kimiskute` e un launcher va in `~/.local/bin`. Vuoi percorsi diversi? Imposta `SUJIRO_VERSION`, `SUJIRO_INSTALL_DIR` o `SUJIRO_BIN_DIR` prima di lanciare lo script.

Ti servirà prima un token di Discord, quindi tieni a portata la sezione seguente mentre l'installer gira.

## Configurazione su Discord

1. Crea un'applicazione sul [portale sviluppatori di Discord](https://discord.com/developers/applications).
2. Aggiungi un bot, poi copia il token e l'Application ID.
3. In OAuth2 > URL Generator, seleziona gli scope `bot` e `applications.commands`.
4. Concedi View Channel, Send Messages, Connect e Speak.
5. Invita il bot con l'URL generato.

## Perché Sujiro?

La maggior parte delle persone vuole solo un bot che suoni musica. Niente pannelli web, niente testi, niente votazioni. Incolla un link e ascolta. Sujiro fa esattamente questo e non ti sta tra i piedi.

Il nome è un gioco di parole dal suono giapponese sulla frase portoghese *"Sugiro que me escute"*, che significa "Ti suggerisco di ascoltarmi". Perfetto per un bot musicale.

## Comandi

| Comando   | Cosa fa                                                     |
| --------- | ---------------------------------------------------------- |
| `/play`   | Riproduce da una ricerca, un URL di un video o una playlist di YouTube |
| `/pause`  | Mette in pausa il brano corrente                           |
| `/resume` | Riprende la riproduzione                                   |
| `/skip`   | Passa al brano successivo                                  |
| `/stop`   | Ferma la riproduzione e svuota la coda                     |
| `/queue`  | Mostra il brano corrente e i 10 successivi                 |
| `/leave`  | Svuota la coda, si disconnette ed elimina lo stato del server |

`/play` richiede che tu sia in un canale vocale. I comandi di controllo (`/pause`, `/resume`, `/skip`, `/stop`) richiedono che tu sia nello stesso canale del bot. Una sessione per server. Il bot se ne va da solo dopo `AUTO_LEAVE_SECONDS` da solo nel canale.

## Compilare dai sorgenti

Preferisci compilarlo tu? Clona il repository e genera il binario di release.

Installa le dipendenze (Ubuntu / Debian):

```bash
sudo apt install -y build-essential pkg-config libopus-dev ffmpeg pipx
pipx ensurepath && pipx install yt-dlp
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust 1.88+
```

Poi clona e compila:

```bash
git clone https://github.com/lexmarcos/sujiro-kimiskute.git
cd sujiro-kimiskute
cp .env.example .env          # inserisci DISCORD_TOKEN e DISCORD_APPLICATION_ID
cargo build --release
./target/release/sujiro-kimiskute
```

Oppure eseguilo con Docker:

```bash
docker build -t sujiro-kimiskute:local .
docker run --rm --env-file .env sujiro-kimiskute:local
```

## Configurazione

Tutte le impostazioni vivono in `.env`. Copia `.env.example` per iniziare, poi inserisci `DISCORD_TOKEN` e `DISCORD_APPLICATION_ID`. Il resto è facoltativo (timeout, dimensione della coda, uscita automatica e altro).

Due impostazioni vale la pena conoscere:

- `BOT_LANGUAGE` imposta la lingua delle descrizioni dei comandi, delle risposte, degli embed e dei controlli. I valori supportati sono `pt-BR` e `en-US`, con `pt-BR` come predefinito se omesso. I nomi dei comandi slash restano in inglese in entrambi i casi.
- `BOT_ACTIVITY_TYPE` e `BOT_ACTIVITY_MESSAGE` impostano la presenza mostrata sul bot. Il tipo distingue maiuscole e minuscole e accetta `playing`, `watching`, `listening` o `competing`. I valori predefiniti sono `listening` e `música`.

Riavvia il bot dopo aver cambiato una di queste impostazioni.

## Token PO di YouTube

Un token Proof of Origin (PO) consente a YouTube di verificare che una richiesta provenga da un client autentico. YouTube li sta imponendo poco a poco. Senza un token, yt-dlp può esporre meno formati, ricevere risposte HTTP 403 o far bloccare temporaneamente l'account o l'IP.

Sujiro si limita a invocare yt-dlp. Non genera né memorizza token PO. La configurazione consigliata è un [plugin PO Token Provider](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) installato sullo stesso host di yt-dlp (o dentro lo stesso container). Con il provider pronto, seleziona il client consigliato `mweb` in `.env`:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb
```

La configurazione manuale è possibile, ma avanzata e sconsigliata:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb;po_token=mweb.gvs+TOKEN
```

Non fare mai commit né logging di token PO o cookie di YouTube. Tienili in `.env` e cambiali subito se trapelano. I token manuali possono essere legati a una sessione o a un singolo video e scadono in fretta, per questo un provider è preferibile. Chi usa Docker deve costruire un'immagine personalizzata che includa il plugin provider e tutte le sue dipendenze di runtime, dato che configurare solo l'host non basta.

Consulta la [guida PO Token](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) e le [note sull'extractor di YouTube](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#youtube) di yt-dlp per i requisiti attuali.

## Architettura

```
discord/   handler di Serenity, comandi slash, embed dell'interfaccia
player/    coda, stato di riproduzione, ciclo di vita della guild, uscita automatica
sources/   risoluzione delle sorgenti (attualmente YouTube via yt-dlp)
voice/     connessione vocale Songbird e gestione degli eventi
config/    configurazione basata sull'ambiente
state/     stato condiviso dell'applicazione
```

La logica specifica di YouTube resta in `sources/youtube/`. Il trait di risoluzione è progettato in modo che Spotify o altre sorgenti si possano aggiungere in seguito senza toccare gli handler dei comandi.

## Limiti (per scelta)

- Solo YouTube (Spotify pianificato)
- Solo comandi slash
- Stato in memoria, perso al riavvio
- Niente database, dashboard web, autoplay, filtri o testi
- Lunghezza della playlist limitata da `MAX_QUEUE_SIZE`
