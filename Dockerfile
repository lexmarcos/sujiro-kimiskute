FROM rust:1.88-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        libopus-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --locked --release

FROM python:3.13-slim-bookworm AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        ffmpeg \
        libopus0 \
    && rm -rf /var/lib/apt/lists/* \
    && python -m pip install --no-cache-dir --disable-pip-version-check yt-dlp \
    && groupadd --gid 10001 musicbot \
    && useradd --uid 10001 --gid musicbot --no-log-init \
        --create-home --home-dir /home/musicbot --shell /bin/false musicbot

COPY --from=builder --chown=musicbot:musicbot \
    /app/target/release/sujiro-kimiskute \
    /usr/local/bin/sujiro-kimiskute

ENV YT_DLP_PATH=/usr/local/bin/yt-dlp

USER musicbot:musicbot
WORKDIR /home/musicbot

CMD ["/usr/local/bin/sujiro-kimiskute"]
