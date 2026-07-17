# Sujiro Kimiskute

<img src="https://i.imgur.com/2xA2jME.png" alt="Sujiro Kimiskute" />

Un bot de música para Discord rápido y ligero, escrito en Rust. Reproduce audio de YouTube mediante comandos de barra. Sin paneles, sin bases de datos, sin peso extra. Solo música.

Es lo bastante ligero para correr en una Raspberry Pi, en un VPS barato o incluso en un móvil Android con [UserLAnd](https://userland.tech/) o [Termux](https://termux.dev/).

**Otros idiomas:** [English](README.md) · [Português](README.pt-BR.md) · [Français](README.fr.md) · [Italiano](README.it.md)

## Instala con un solo comando

En cualquier sistema Linux de 64 bits esto es todo lo que necesitas. El instalador detecta tu procesador (x86_64 o ARM64), descarga y verifica la release correspondiente, instala `yt-dlp` si falta y te guía en la creación del archivo `.env`:

```bash
curl -fsSLO https://raw.githubusercontent.com/lexmarcos/sujiro-kimiskute/main/install.sh
chmod +x install.sh
./install.sh
```

Listo. El bot queda en `~/.local/share/sujiro-kimiskute` y un lanzador va a `~/.local/bin`. ¿Quieres otras rutas? Define `SUJIRO_VERSION`, `SUJIRO_INSTALL_DIR` o `SUJIRO_BIN_DIR` antes de ejecutar el script.

Necesitarás un token de Discord antes, así que ten a mano la siguiente sección mientras corre el instalador.

## Configuración en Discord

1. Crea una aplicación en el [Portal de Desarrolladores de Discord](https://discord.com/developers/applications).
2. Añade un bot y copia el token y el Application ID.
3. En OAuth2 > URL Generator, marca los ámbitos `bot` y `applications.commands`.
4. Concede View Channel, Send Messages, Connect y Speak.
5. Invita al bot con la URL generada.

## ¿Por qué Sujiro?

La mayoría de la gente solo quiere un bot que reproduzca música. Sin paneles web, sin letras, sin votaciones. Pega un enlace y escucha. Sujiro hace justo eso y no se mete en tu camino.

El nombre es un juego de palabras con sonido japonés sobre la frase en portugués *"Sugiro que me escute"*, que significa "Sugiero que me escuches". Muy apropiado para un bot de música.

## Comandos

| Comando   | Qué hace                                                    |
| --------- | ----------------------------------------------------------- |
| `/play`   | Reproduce desde una búsqueda, una URL de vídeo o una lista de YouTube |
| `/pause`  | Pausa la pista actual                                       |
| `/resume` | Reanuda la reproducción                                     |
| `/skip`   | Salta a la siguiente pista                                  |
| `/stop`   | Detiene la reproducción y vacía la cola                     |
| `/queue`  | Muestra la pista actual y las 10 siguientes                 |
| `/leave`  | Vacía la cola, se desconecta y descarta el estado del servidor |

`/play` requiere que estés en un canal de voz. Los comandos de control (`/pause`, `/resume`, `/skip`, `/stop`) requieren que estés en el mismo canal que el bot. Una sesión por servidor. El bot se sale solo tras `AUTO_LEAVE_SECONDS` a solas en el canal.

## Compilar desde el código

¿Prefieres compilarlo tú mismo? Clona el repositorio y genera el binario de release.

Instala las dependencias (Ubuntu / Debian):

```bash
sudo apt install -y build-essential pkg-config libopus-dev ffmpeg pipx
pipx ensurepath && pipx install yt-dlp
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust 1.88+
```

Luego clona y compila:

```bash
git clone https://github.com/lexmarcos/sujiro-kimiskute.git
cd sujiro-kimiskute
cp .env.example .env          # completa DISCORD_TOKEN y DISCORD_APPLICATION_ID
cargo build --release
./target/release/sujiro-kimiskute
```

O ejecútalo con Docker:

```bash
docker build -t sujiro-kimiskute:local .
docker run --rm --env-file .env sujiro-kimiskute:local
```

## Configuración

Todos los ajustes viven en `.env`. Copia `.env.example` para empezar y completa `DISCORD_TOKEN` y `DISCORD_APPLICATION_ID`. Lo demás es opcional (tiempos de espera, tamaño de la cola, salida automática y más).

Dos ajustes vale la pena conocer:

- `BOT_LANGUAGE` define el idioma de las descripciones de comandos, respuestas, embeds y controles. Los valores admitidos son `pt-BR` y `en-US`, con `pt-BR` por defecto si se omite. Los nombres de los comandos de barra siguen en inglés en ambos casos.
- `BOT_ACTIVITY_TYPE` y `BOT_ACTIVITY_MESSAGE` definen la presencia que muestra el bot. El tipo distingue mayúsculas y minúsculas y admite `playing`, `watching`, `listening` o `competing`. Los valores por defecto son `listening` y `música`.

Reinicia el bot después de cambiar cualquiera de estos.

## Tokens PO de YouTube

Un token de Proof of Origin (PO) permite que YouTube verifique que la petición vino de un cliente legítimo. YouTube lo está aplicando poco a poco. Sin un token, yt-dlp puede exponer menos formatos, recibir respuestas HTTP 403 o hacer que la cuenta o la IP queden bloqueadas de forma temporal.

Sujiro solo invoca yt-dlp. No genera ni almacena tokens PO. La configuración recomendada es un [plugin de PO Token Provider](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) instalado en el mismo host que yt-dlp (o dentro del mismo contenedor). Con el provider listo, selecciona el cliente recomendado `mweb` en `.env`:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb
```

La configuración manual es posible, pero avanzada y no recomendada:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb;po_token=mweb.gvs+TOKEN
```

Nunca subas ni registres en logs tokens PO o cookies de YouTube. Guárdalos en `.env` y cámbialos de inmediato si se filtran. Los tokens manuales pueden estar ligados a una sesión o a un solo vídeo y expiran rápido, por eso se prefiere un provider. Quien use Docker debe construir una imagen propia que incluya el plugin provider y todas sus dependencias de runtime, ya que configurar solo el host no basta.

Consulta la [Guía de PO Token](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) y las [notas del extractor de YouTube](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#youtube) de yt-dlp para los requisitos actuales.

## Arquitectura

```
discord/   handlers de Serenity, comandos de barra, embeds de la UI
player/    cola, estado de reproducción, ciclo de vida de la guild, salida automática
sources/   resolución de fuentes (actualmente YouTube vía yt-dlp)
voice/     conexión de voz de Songbird y manejo de eventos
config/    configuración basada en el entorno
state/     estado compartido de la aplicación
```

La lógica específica de YouTube se queda en `sources/youtube/`. El trait de resolución está pensado para que Spotify u otras fuentes se puedan añadir más adelante sin tocar los handlers de comandos.

## Limitaciones (por diseño)

- Solo YouTube (Spotify planeado)
- Solo comandos de barra
- Estado en memoria, se pierde al reiniciar
- Sin base de datos, panel web, autoplay, filtros ni letras
- Longitud de la lista limitada por `MAX_QUEUE_SIZE`
