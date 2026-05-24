# =============================================================================
#  cyberdeck-emu — image Docker minimaliste
# =============================================================================
#
#  Build multi-stage :
#    1. `builder`  — image rust:slim, compile en release avec
#                    `--no-default-features` pour exclure serialport/libudev.
#    2. `runtime`  — image debian:bookworm-slim, copie juste le binaire.
#
#  Le binaire pèse ~5 MB une fois strippé.  Image finale ~85 MB (Debian
#  slim) ou ~12 MB si on bascule sur gcr.io/distroless/cc-debian12 (à
#  envisager quand on aura confirmé tous les chemins runtime).
#
#  Construire :   docker build -t cyberdeck-emu .
#  Lancer :       docker run -p 17017:17017 cyberdeck-emu
#  Via compose :  docker compose up -d
# =============================================================================

FROM rust:1.91-slim AS builder

WORKDIR /build
# On copie le workspace entier — Cargo verra les autres crates mais ne les
# touchera pas grâce au filtre `-p cyberdeck-emu` et `--no-default-features`
# (donc pas besoin de libudev-dev ni libusb).
COPY Cargo.toml ./
COPY crates ./crates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release -p cyberdeck-emu --no-default-features \
    && cp /build/target/release/cyberdeck-emu /usr/local/bin/cyberdeck-emu \
    && strip /usr/local/bin/cyberdeck-emu

# =============================================================================
FROM debian:bookworm-slim AS runtime

# Compte non-root (sécurité Docker : éviter root inutilement).
RUN groupadd -r emu --gid 1000 \
 && useradd -r -g emu --uid 1000 --no-create-home --home-dir /nonexistent emu

COPY --from=builder /usr/local/bin/cyberdeck-emu /usr/local/bin/cyberdeck-emu

USER emu

EXPOSE 17017

ENV RUST_LOG=info

# Healthcheck : le port doit accepter une connexion TCP.
# `bash` n'est pas installé en slim ; on utilise un script Python si
# disponible — sinon `nc` (netcat-openbsd dans la slim ? non).
# Pour rester portable, on omet le HEALTHCHECK ici ; docker-compose
# fera la sienne via le pattern `nc -z`.

ENTRYPOINT ["/usr/local/bin/cyberdeck-emu"]
CMD ["--listen", "0.0.0.0:17017"]
