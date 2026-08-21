# Sujiro Kimiskute

<img src="https://i.imgur.com/2xA2jME.png" alt="Sujiro Kimiskute" />

Um bot de música para Discord rápido e leve, escrito em Rust. Ele toca áudio do YouTube por comandos de barra. Sem painéis, sem bancos de dados, sem peso extra. Só música.

Ele é leve o suficiente para rodar em um Raspberry Pi, em um VPS barato ou até em um celular Android pelo [UserLAnd](https://userland.tech/) ou pelo [Termux](https://termux.dev/).

**Outros idiomas:** [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Italiano](README.it.md)

## Instale com um comando

Em qualquer sistema Linux de 64 bits é só isso que você precisa. O instalador detecta seu processador (x86_64 ou ARM64), baixa e verifica a release correspondente, instala o `yt-dlp` caso esteja faltando (o zipapp oficial quando há Python 3.10+, o binário standalone caso contrário), ajusta as opções do yt-dlp ao seu host e conduz você na criação do arquivo `.env`:

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
pipx ensurepath && pipx install 'yt-dlp[default]'
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

Opções que valem a pena conhecer:

- `BOT_LANGUAGE` define o idioma das descrições de comandos, respostas, embeds e controles. Os valores aceitos são `pt-BR` e `en-US`, com padrão `pt-BR` quando omitido. Os nomes dos comandos de barra continuam em inglês nos dois casos.
- `BOT_ACTIVITY_TYPE` e `BOT_ACTIVITY_MESSAGE` definem a presença exibida no bot. O tipo diferencia maiúsculas e minúsculas e aceita `playing`, `watching`, `listening` ou `competing`. Os padrões são `listening` e `música`.
- `YT_DLP_EXTRA_ARGS` repassa flags extras ao yt-dlp (aspas no estilo shell). O bot sempre adiciona `skip=hls,dash` aos seus extractor args de `youtube:`, porque só toca streams HTTP simples. Mantenha toda configuração do YouTube em um único valor `--extractor-args youtube:...`: o yt-dlp só guarda o último por extractor, e o valor que o bot passa também sobrescreve uma seção `youtube:` de um arquivo de configuração do yt-dlp.
- `YT_DLP_TIMEOUT_SECONDS` (padrão `45`) limita cada execução do yt-dlp. O desafio JS do YouTube leva cerca de dois segundos de CPU num desktop e dez vezes isso numa placa ARM fraca; use `60` nelas.
- `MAX_CONCURRENT_RESOLUTIONS` (padrão `2`) limita execuções paralelas do yt-dlp. Cada uma pode chegar perto de 300 MB quando há um runtime JavaScript envolvido; use `1` em hosts com uma vCPU ou 1 GB de RAM.

Reinicie o bot depois de mudar qualquer uma dessas opções.

Enquanto uma faixa toca, o stream da próxima faixa da fila é preparado em segundo plano, então a transição não espera o yt-dlp. A pré-resolução só roda quando há uma vaga de resolução livre.

## Hosts com poucos recursos

O Sujiro em si é barato: o áudio Opus do YouTube é repassado direto ao Discord sem decodificar. Quase toda CPU, memória e espera vão para o `yt-dlp`, e três escolhas decidem quanto:

1. **Runtime JavaScript.** O yt-dlp recente resolve o desafio do player do YouTube com Deno (ou Node via `--js-runtimes node`). Medido num desktop, uma resolução custa cerca de 0,6 s de CPU e 60 MB sem runtime contra 2 s de CPU, 300 MB e uma requisição a mais com ele; placas ARM fracas são cerca de dez vezes mais lentas. Sem runtime o yt-dlp cai num único cliente que não precisa do desafio. Funciona hoje, mas o yt-dlp marca isso como obsoleto e o YouTube muda quais clientes funcionam sem PO token. Num host fraco, comece sem runtime e adicione o Deno (ou `--js-runtimes node` em `YT_DLP_EXTRA_ARGS`) só quando a reprodução quebrar.
2. **Requisições por faixa.** O bot já pula os manifestos HLS e DASH. `player_skip=webpage,configs` também pula a página de 1,4 MB e reduz a saída do yt-dlp de cerca de 600 KB para 100 KB de JSON:

   ```dotenv
   YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_skip=webpage,configs
   YT_DLP_TIMEOUT_SECONDS=60
   MAX_CONCURRENT_RESOLUTIONS=1
   ```

   O instalador aplica esse perfil em hosts com até 2 CPUs ou 2 GB de RAM. Remova `player_skip` se você usa PO tokens com clientes web: eles precisam da página.
3. **Como o yt-dlp é instalado.** `pip install 'yt-dlp[default]'` (ou pipx) inicia em cerca de 0,15 s num desktop, o zipapp em 0,5 s e o binário standalone `yt-dlp_linux` em 0,6 s, extraindo 40 MB em `/tmp` a cada execução, o que dói em armazenamento lento e sob proot (UserLAnd). O instalador prefere o zipapp sempre que há Python 3.10+. Mantenha o yt-dlp atualizado (`yt-dlp -U`) e mantenha `~/.cache/yt-dlp` gravável e persistente: ele guarda as assinaturas de player já resolvidas.

Meça no seu próprio aparelho com `RUST_LOG=info`: toda resolução registra `yt-dlp process finished` com seu `duration_ms`.

Quando o yt-dlp avisa alguma coisa — falta de runtime JavaScript, um cliente que exige PO token, mudança de extractor — o bot registra como `yt-dlp reported diagnostics`, mesmo quando a execução deu certo. Esses avisos costumam ser a única pista quando a resolução funciona mas a reprodução falha depois com erro HTTP, então leia-os antes de suspeitar do bot. URLs neles têm a query string redigida, já que URLs de mídia assinadas carregam PO token e assinatura.

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

Mantenha toda configuração de `youtube:` nesse único valor (o yt-dlp só guarda o último `--extractor-args` por extractor; o bot mescla o próprio `skip=hls,dash` ao seu) e não combine com `player_skip=webpage`: PO tokens de clientes web precisam da página.

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
