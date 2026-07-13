# Sujiro Kimiskute

Bot de música minimalista para Discord, escrito em Rust e controlado somente por slash commands. Esta versão reproduz buscas, vídeos e playlists do YouTube com `yt-dlp`, mantém uma fila independente por servidor e guarda todo o estado apenas em memória.

## Requisitos

- Rust 1.88 ou mais recente;
- `yt-dlp` disponível no `PATH` ou indicado por `YT_DLP_PATH`;
- `pkg-config` e a biblioteca de desenvolvimento do Opus;
- FFmpeg;
- uma aplicação Discord com um bot configurado.

### Instalação no Ubuntu

Instale as dependências nativas e o `pipx`:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libopus-dev ffmpeg pipx
pipx ensurepath
```

Abra um novo shell após `pipx ensurepath` e instale o `yt-dlp`:

```bash
pipx install yt-dlp
yt-dlp --version
```

Instale o Rust pelo `rustup`, caso ainda não esteja disponível:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup default stable
rustc --version
```

Confirme que a versão exibida é 1.88 ou superior.

## Configuração do Discord

1. Acesse o [Discord Developer Portal](https://discord.com/developers/applications) e crie uma aplicação.
2. Na seção **Bot**, adicione um bot e copie o token. Nunca publique ou faça commit desse valor.
3. Copie o **Application ID** na página **General Information**.
4. Em **OAuth2 > URL Generator**, selecione os scopes `bot` e `applications.commands`.
5. Conceda ao bot somente estas permissões:
   - View Channel;
   - Send Messages;
   - Connect;
   - Speak.
6. Abra a URL gerada e adicione o bot ao servidor desejado.

O cliente solicita somente os gateway intents `GUILDS` e `GUILD_VOICE_STATES`. Ele não usa Message Content. O estado de voz não possui um toggle privilegiado separado no Developer Portal.

Os comandos são registrados globalmente quando o bot fica pronto. Alterações em comandos globais podem levar algum tempo para aparecer em todos os servidores.

## Variáveis de ambiente

Crie o arquivo local de configuração:

```bash
cp .env.example .env
```

Preencha pelo menos as duas variáveis obrigatórias:

```dotenv
DISCORD_TOKEN=cole_o_token_do_bot_aqui
DISCORD_APPLICATION_ID=cole_o_application_id_aqui

YT_DLP_PATH=yt-dlp
YT_DLP_EXTRA_ARGS=
YT_DLP_TIMEOUT_SECONDS=20
AUTO_LEAVE_SECONDS=120
MAX_QUEUE_SIZE=50
MAX_CONCURRENT_RESOLUTIONS=4
RUST_LOG=info
```

| Variável | Obrigatória | Default | Finalidade |
| --- | --- | --- | --- |
| `DISCORD_TOKEN` | Sim | — | Token secreto do bot. |
| `DISCORD_APPLICATION_ID` | Sim | — | ID numérico da aplicação Discord. |
| `YT_DLP_PATH` | Não | `yt-dlp` | Caminho ou nome do executável. |
| `YT_DLP_EXTRA_ARGS` | Não | vazio | Argumentos adicionais separados com sintaxe compatível com `shlex`; cada argumento é enviado diretamente ao processo, sem shell. |
| `YT_DLP_TIMEOUT_SECONDS` | Não | `20` | Timeout de cada execução do `yt-dlp`. |
| `AUTO_LEAVE_SECONDS` | Não | `120` | Tempo sozinho no canal antes da desconexão automática. |
| `MAX_QUEUE_SIZE` | Não | `50` | Quantidade máxima de faixas aguardando por servidor e limite de resolução de playlist. |
| `MAX_CONCURRENT_RESOLUTIONS` | Não | `4` | Limite global de processos de resolução simultâneos. |
| `RUST_LOG` | Não | `info` | Filtro do `tracing`, por exemplo `info` ou `sujiro_kimiskute=debug`. |

Todos os valores numéricos configuráveis devem ser positivos. Variáveis de ambiente do processo têm precedência sobre o arquivo `.env`.

## Execução

Em desenvolvimento:

```bash
cargo run
```

Para gerar e executar o binário otimizado:

```bash
cargo build --release
./target/release/sujiro-kimiskute
```

Use `Ctrl+C` para encerrar o cliente Discord de forma controlada.

## Comandos

- `/play <query>` — aceita texto de busca, URL de vídeo ou URL de playlist do YouTube; conecta ao canal do usuário e adiciona o resultado à fila.
- `/pause` — pausa a faixa atual.
- `/resume` — retoma a faixa pausada.
- `/skip` — encerra a faixa atual e avança uma vez.
- `/stop` — interrompe a reprodução e limpa a fila, mantendo o bot conectado.
- `/queue` — mostra a faixa atual e até dez próximas; pode ser usado por qualquer membro do servidor, mesmo fora do canal de voz.
- `/leave` — interrompe a sessão, limpa a fila, desconecta o bot e remove o estado temporário do servidor.

`/play` exige que o usuário esteja em um canal de voz. Os comandos de controle exigem que o usuário esteja no mesmo canal do bot. Há somente uma sessão de reprodução por servidor.

Quando o bot fica sem usuários humanos no canal, inicia o timeout de auto-leave. A contagem é cancelada se alguém retornar ou iniciar uma nova atividade válida antes do prazo.

Buscas retornam inicialmente um resultado. URLs de vídeo resolvem uma faixa. Playlists são limitadas por `MAX_QUEUE_SIZE`; se houver menos espaço livre na fila, somente o prefixo que couber é adicionado, preservando a ordem, e o bot informa quantas faixas foram omitidas.

## Logs

Os logs usam `tracing` e incluem contexto operacional como servidor, usuário, canal, faixa, duração de resolução e mudanças de reprodução. Ajuste o nível com `RUST_LOG`.

Tokens do Discord, cookies, PO Tokens, argumentos extras completos e URLs sensíveis não devem ser colocados em logs. Não inclua segredos no repositório; o arquivo `.env` já é ignorado pelo Git.

## PO Token e opções avançadas do yt-dlp

O núcleo do bot não gera PO Tokens. Um PO Token provider ou plugin compatível deve ser instalado e mantido no mesmo ambiente do `yt-dlp`; isso é uma dependência de infraestrutura.

Argumentos adicionais podem ser fornecidos sem alterar o código. Por exemplo:

```dotenv
YT_DLP_EXTRA_ARGS=--extractor-args youtube:player_client=mweb
```

O bot passa esses argumentos diretamente ao `yt-dlp`. Não grave PO Tokens, cookies ou outros segredos no código, no README ou na imagem de produção.

## Docker

Construa a imagem multi-stage:

```bash
docker build -t sujiro-kimiskute:local .
```

Inicie o bot fornecendo as variáveis externamente:

```bash
docker run --rm \
  --name sujiro-kimiskute \
  --env-file .env \
  sujiro-kimiskute:local
```

A imagem executa como usuário não root e já contém `yt-dlp`, FFmpeg, certificados TLS e a biblioteca Opus. O caminho padrão do executável dentro do container é `/usr/local/bin/yt-dlp`; não inclua tokens, cookies ou outros segredos no build ou na imagem. Providers externos de PO Token não são incluídos e exigem uma imagem derivada ou outro mecanismo de infraestrutura.

Não há `HEALTHCHECK`: o bot não expõe endpoint HTTP ou outro indicador local que represente de forma confiável sua conexão com Discord e voz.

## Limitações atuais

- somente YouTube; Spotify poderá ser adicionado futuramente;
- somente slash commands;
- estado e filas apenas em memória, perdidos ao reiniciar;
- sem banco de dados, dashboard ou cache persistente;
- sem autoplay, filtros, equalizador, normalização, letras ou download completo de áudio;
- playlists limitadas pelo tamanho configurado da fila;
- funcionamento sujeito à disponibilidade do YouTube, do `yt-dlp` e de eventuais providers externos;
- esta etapa não inclui testes unitários, de integração, snapshots ou mocks.

Para validar o código sem testes:

```bash
cargo fmt --check
cargo check
cargo clippy -- -D warnings
cargo build --release
```
