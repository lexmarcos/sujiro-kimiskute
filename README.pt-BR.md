# Sujiro Kimiskute

<img src="https://i.imgur.com/2xA2jME.png" alt="Sujiro Kimiskute" />

Um bot de música para Discord rápido e leve, escrito em Rust. Ele toca áudio do YouTube por comandos de barra. Sem painéis, sem bancos de dados, sem peso extra. Só música.

Ele é leve o suficiente para rodar em um Raspberry Pi, em um VPS barato ou até em um celular Android pelo [UserLAnd](https://userland.tech/) ou pelo [Termux](https://termux.dev/).

**Outros idiomas:** [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Italiano](README.it.md)

## Instale com um comando

Em qualquer sistema Linux de 64 bits é só isso que você precisa. O instalador detecta seu processador (x86_64 ou ARM64), baixa e verifica a release correspondente, instala o `yt-dlp` caso esteja faltando e conduz você na criação do arquivo `.env`:

```bash
curl -fsSLO https://raw.githubusercontent.com/lexmarcos/sujiro-kimiskute/main/install.sh
chmod +x install.sh
./install.sh
```

Pronto. O bot vai para `~/.local/share/sujiro-kimiskute` e um lançador vai para `~/.local/bin`. Quer outros caminhos? Defina `SUJIRO_VERSION`, `SUJIRO_INSTALL_DIR` ou `SUJIRO_BIN_DIR` antes de rodar o script.

Você vai precisar de um token do Discord antes, então deixe a próxima seção por perto enquanto o instalador roda.

## Configuração no Discord

1. Crie uma aplicação no [Portal de Desenvolvedores do Discord](https://discord.com/developers/applications).
2. Adicione um bot e copie o token e o Application ID.
3. Em OAuth2 > URL Generator, marque os escopos `bot` e `applications.commands`.
4. Conceda View Channel, Send Messages, Connect e Speak.
5. Convide o bot pela URL gerada.

## Por que o Sujiro?

A maioria das pessoas só quer um bot que toque música. Sem painéis web, sem letras, sem votação. Cole um link e escute. O Sujiro faz exatamente isso e não fica no seu caminho.

O nome é um trocadilho de sonoridade japonesa com a frase em português *"Sugiro que me escute"*. Bem apropriado para um bot de música.

## Comandos

| Comando   | O que faz                                                  |
| --------- | ---------------------------------------------------------- |
| `/play`   | Toca a partir de uma busca, de uma URL de vídeo ou de uma playlist do YouTube |
| `/pause`  | Pausa a faixa atual                                        |
| `/resume` | Retoma a reprodução                                        |
| `/skip`   | Pula para a próxima faixa                                  |
| `/stop`   | Para a reprodução e limpa a fila                           |
| `/queue`  | Mostra a faixa atual e as próximas 10                      |
| `/leave`  | Limpa a fila, desconecta e descarta o estado do servidor  |

O `/play` exige que você esteja em um canal de voz. Os comandos de controle (`/pause`, `/resume`, `/skip`, `/stop`) exigem que você esteja no mesmo canal do bot. Uma sessão por servidor. O bot sai sozinho depois de `AUTO_LEAVE_SECONDS` sozinho no canal.

## Compilar a partir do código

Prefere compilar você mesmo? Clone o repositório e gere o binário de release.

Instale as dependências (Ubuntu / Debian):

```bash
sudo apt install -y build-essential pkg-config libopus-dev ffmpeg pipx
pipx ensurepath && pipx install yt-dlp
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust 1.88+
```

Depois clone e compile:

```bash
git clone https://github.com/lexmarcos/sujiro-kimiskute.git
cd sujiro-kimiskute
cp .env.example .env          # preencha DISCORD_TOKEN e DISCORD_APPLICATION_ID
cargo build --release
./target/release/sujiro-kimiskute
```

Ou rode com Docker:

```bash
docker build -t sujiro-kimiskute:local .
docker run --rm --env-file .env sujiro-kimiskute:local
```

## Configuração

Todas as opções ficam no `.env`. Copie o `.env.example` para começar e preencha `DISCORD_TOKEN` e `DISCORD_APPLICATION_ID`. O resto é opcional (tempos limite, tamanho da fila, saída automática e mais).

Duas opções valem a pena conhecer:

- `BOT_LANGUAGE` define o idioma das descrições de comandos, respostas, embeds e controles. Os valores aceitos são `pt-BR` e `en-US`, com padrão `pt-BR` quando omitido. Os nomes dos comandos de barra continuam em inglês nos dois casos.
- `BOT_ACTIVITY_TYPE` e `BOT_ACTIVITY_MESSAGE` definem a presença exibida no bot. O tipo diferencia maiúsculas e minúsculas e aceita `playing`, `watching`, `listening` ou `competing`. Os padrões são `listening` e `música`.

Reinicie o bot depois de mudar qualquer uma dessas opções.

## Tokens PO do YouTube

Um token de Proof of Origin (PO) permite que o YouTube verifique que a requisição veio de um cliente legítimo. O YouTube está aplicando isso aos poucos. Sem um token, o yt-dlp pode expor menos formatos, receber respostas HTTP 403 ou ter a conta ou o IP bloqueados temporariamente.

O Sujiro apenas chama o yt-dlp. Ele não gera nem armazena tokens PO. A configuração recomendada é um [plugin de PO Token Provider](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) instalado no mesmo host do yt-dlp (ou dentro do mesmo contêiner). Com o provider pronto, selecione o cliente recomendado `mweb` no `.env`:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb
```

A configuração manual é possível, mas avançada e não recomendada:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb;po_token=mweb.gvs+TOKEN
```

Nunca faça commit nem registre em log tokens PO ou cookies do YouTube. Mantenha tudo no `.env` e troque na hora se vazar. Tokens manuais podem estar ligados a uma sessão ou a um único vídeo e expiram rápido, por isso um provider é preferível. Quem usa Docker precisa construir uma imagem própria com o plugin provider e todas as suas dependências de runtime, já que configurar só o host não basta.

Consulte o [Guia de PO Token](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) e as [notas do extractor do YouTube](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#youtube) do yt-dlp para os requisitos atuais.

## Arquitetura

```
discord/   handlers do Serenity, comandos de barra, embeds da UI
player/    fila, estado de reprodução, ciclo de vida da guild, saída automática
sources/   resolução de fontes (atualmente YouTube via yt-dlp)
voice/     conexão de voz do Songbird e tratamento de eventos
config/    configuração baseada em ambiente
state/     estado compartilhado da aplicação
```

A lógica específica do YouTube fica em `sources/youtube/`. O trait de resolução foi pensado para que Spotify ou outras fontes possam ser adicionados depois sem mexer nos handlers de comando.

## Limitações (por design)

- Só YouTube (Spotify planejado)
- Só comandos de barra
- Estado em memória, perdido ao reiniciar
- Sem banco de dados, painel web, autoplay, filtros ou letras
- Tamanho da playlist limitado por `MAX_QUEUE_SIZE`
