# syntax=docker/dockerfile:1
#
# The app.timer online relay, as one image.
#
# The build context is the *repository root*, not this directory, because the
# image needs both the relay crate (relay/) and the frontend it serves (the
# Vite project at the root). Build it with:
#
#   docker build -f relay/Dockerfile -t app-timer-relay .
#
# from the repository root, or use relay/docker-compose.yml, which sets the
# context for you.

# ---------------------------------------------------------------------------
# Stage 1 - build the remote web application that the relay serves
# ---------------------------------------------------------------------------
FROM node:22-alpine AS web

WORKDIR /app

# Dependencies first, so a source edit does not reinstall them.
COPY package.json package-lock.json ./
RUN npm ci

COPY index.html vite.config.ts tsconfig.json tsconfig.node.json ./
COPY src ./src
RUN npm run build

# ---------------------------------------------------------------------------
# Stage 2 - build the relay binary
# ---------------------------------------------------------------------------
FROM rust:1.85-bookworm AS relay

WORKDIR /src

# Compile the dependency tree against a stub, so a change to our own sources
# does not rebuild every crate on the next build.
COPY relay/Cargo.toml relay/Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY relay/src ./src
RUN touch src/main.rs && cargo build --release --locked

# ---------------------------------------------------------------------------
# Stage 3 - runtime
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# The relay never needs to write to disk, so it runs as an unprivileged user
# with no home directory.
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin relay

WORKDIR /srv

COPY --from=relay /src/target/release/app-timer-relay /usr/local/bin/app-timer-relay
COPY --from=web /app/dist ./dist

USER relay

ENV RELAY_BIND=0.0.0.0:8443 \
    RELAY_WEB_ROOT=/srv/dist \
    RELAY_LOG=info

# The listener port. TLS is terminated here, so this must be reachable by both
# the Windows host (outbound) and remote devices (inbound).
EXPOSE 8443

# Configuration and secrets come from the environment only; nothing is baked
# into the image.
ENTRYPOINT ["/usr/local/bin/app-timer-relay"]
