# Sujiro Kimiskute

<img src="https://i.imgur.com/2xA2jME.png" alt="Sujiro Kimiskute" />

Un bot de musique Discord rapide et léger, écrit en Rust. Il joue l'audio de YouTube via des commandes slash. Pas de tableaux de bord, pas de bases de données, aucun poids inutile. Juste de la musique.

Il est assez léger pour tourner sur un Raspberry Pi, sur un VPS bon marché ou même sur un téléphone Android avec [UserLAnd](https://userland.tech/) ou [Termux](https://termux.dev/).

**Autres langues :** [English](README.md) · [Português](README.pt-BR.md) · [Español](README.es.md) · [Italiano](README.it.md)

## Installez en une commande

Sur n'importe quel système Linux 64 bits, c'est tout ce dont vous avez besoin. L'installeur détecte votre processeur (x86_64 ou ARM64), télécharge et vérifie la release correspondante, installe `yt-dlp` s'il manque et vous accompagne dans la création du fichier `.env` :

```bash
curl -fsSLO https://raw.githubusercontent.com/lexmarcos/sujiro-kimiskute/main/install.sh
chmod +x install.sh
./install.sh
```

Voilà. Le bot se place dans `~/.local/share/sujiro-kimiskute` et un lanceur va dans `~/.local/bin`. Vous voulez d'autres chemins ? Définissez `SUJIRO_VERSION`, `SUJIRO_INSTALL_DIR` ou `SUJIRO_BIN_DIR` avant de lancer le script.

Il vous faudra d'abord un token Discord, alors gardez la section suivante sous la main pendant que l'installeur tourne.

## Configuration Discord

1. Créez une application sur le [portail développeur de Discord](https://discord.com/developers/applications).
2. Ajoutez un bot, puis copiez le token et l'Application ID.
3. Dans OAuth2 > URL Generator, cochez les scopes `bot` et `applications.commands`.
4. Accordez View Channel, Send Messages, Connect et Speak.
5. Invitez le bot avec l'URL générée.

## Pourquoi Sujiro ?

La plupart des gens veulent juste un bot qui joue de la musique. Pas de panneaux web, pas de paroles, pas de votes. Collez un lien et écoutez. Sujiro fait exactement cela et vous laisse tranquille.

Le nom est un jeu de mots à consonance japonaise sur la phrase portugaise *"Sugiro que me escute"*, qui signifie "Je vous suggère de m'écouter". Bien choisi pour un bot de musique.

## Commandes

| Commande  | Ce qu'elle fait                                              |
| --------- | ----------------------------------------------------------- |
| `/play`   | Joue depuis une recherche, une URL de vidéo ou une playlist YouTube |
| `/pause`  | Met en pause la piste en cours                              |
| `/resume` | Reprend la lecture                                          |
| `/skip`   | Passe à la piste suivante                                   |
| `/stop`   | Arrête la lecture et vide la file                           |
| `/queue`  | Affiche la piste en cours et les 10 suivantes               |
| `/leave`  | Vide la file, se déconnecte et supprime l'état du serveur   |

`/play` exige que vous soyez dans un salon vocal. Les commandes de contrôle (`/pause`, `/resume`, `/skip`, `/stop`) exigent que vous soyez dans le même salon que le bot. Une session par serveur. Le bot quitte de lui-même après `AUTO_LEAVE_SECONDS` seul dans le salon.

## Compiler depuis les sources

Vous préférez compiler vous-même ? Clonez le dépôt et générez le binaire de release.

Installez les dépendances (Ubuntu / Debian) :

```bash
sudo apt install -y build-essential pkg-config libopus-dev ffmpeg pipx
pipx ensurepath && pipx install yt-dlp
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust 1.88+
```

Puis clonez et compilez :

```bash
git clone https://github.com/lexmarcos/sujiro-kimiskute.git
cd sujiro-kimiskute
cp .env.example .env          # renseignez DISCORD_TOKEN et DISCORD_APPLICATION_ID
cargo build --release
./target/release/sujiro-kimiskute
```

Ou lancez-le avec Docker :

```bash
docker build -t sujiro-kimiskute:local .
docker run --rm --env-file .env sujiro-kimiskute:local
```

## Configuration

Tous les réglages vivent dans `.env`. Copiez `.env.example` pour commencer, puis renseignez `DISCORD_TOKEN` et `DISCORD_APPLICATION_ID`. Le reste est facultatif (délais, taille de la file, départ automatique et plus encore).

Deux réglages valent la peine d'être connus :

- `BOT_LANGUAGE` définit la langue des descriptions de commandes, des réponses, des embeds et des contrôles. Les valeurs acceptées sont `pt-BR` et `en-US`, avec `pt-BR` par défaut si elle est omise. Les noms des commandes slash restent en anglais dans les deux cas.
- `BOT_ACTIVITY_TYPE` et `BOT_ACTIVITY_MESSAGE` définissent la présence affichée sur le bot. Le type est sensible à la casse et accepte `playing`, `watching`, `listening` ou `competing`. Les valeurs par défaut sont `listening` et `música`.

Redémarrez le bot après avoir changé l'un de ces réglages.

## Tokens PO de YouTube

Un token Proof of Origin (PO) permet à YouTube de vérifier qu'une requête vient d'un client authentique. YouTube les impose progressivement. Sans token, yt-dlp peut exposer moins de formats, recevoir des réponses HTTP 403 ou faire bloquer temporairement le compte ou l'IP.

Sujiro ne fait qu'appeler yt-dlp. Il ne génère ni ne stocke de tokens PO. La configuration recommandée est un [plugin PO Token Provider](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) installé sur le même hôte que yt-dlp (ou dans le même conteneur). Une fois le provider prêt, sélectionnez le client recommandé `mweb` dans `.env` :

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb
```

La configuration manuelle est possible, mais avancée et déconseillée :

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb;po_token=mweb.gvs+TOKEN
```

Ne validez jamais et ne journalisez jamais les tokens PO ou les cookies YouTube. Gardez-les dans `.env` et changez-les tout de suite en cas de fuite. Les tokens manuels peuvent être liés à une session ou à une seule vidéo et expirent vite, c'est pourquoi un provider est préférable. Les utilisateurs de Docker doivent construire une image sur mesure qui embarque le plugin provider et toutes ses dépendances d'exécution, car configurer l'hôte seul ne suffit pas.

Consultez le [guide PO Token](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) et les [notes sur l'extracteur YouTube](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#youtube) de yt-dlp pour les exigences actuelles.

## Architecture

```
discord/   handlers Serenity, commandes slash, embeds d'interface
player/    file, état de lecture, cycle de vie de la guild, départ automatique
sources/   résolution des sources (actuellement YouTube via yt-dlp)
voice/     connexion vocale Songbird et gestion des événements
config/    configuration basée sur l'environnement
state/     état partagé de l'application
```

La logique propre à YouTube reste dans `sources/youtube/`. Le trait de résolution est conçu pour que Spotify ou d'autres sources puissent être ajoutés plus tard sans toucher aux handlers de commandes.

## Limites (par choix)

- YouTube uniquement (Spotify prévu)
- Commandes slash uniquement
- État en mémoire, perdu au redémarrage
- Pas de base de données, de tableau de bord web, de lecture automatique, de filtres ni de paroles
- Longueur de playlist limitée par `MAX_QUEUE_SIZE`
