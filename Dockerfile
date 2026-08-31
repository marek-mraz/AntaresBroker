# Multi-stage: build on the target arch (arm64/amd64 both fine), run distroless
# (non-root, read-only rootfs friendly).
#
# cargo-chef splits the build so the ~400 dependency crates compile in their
# own layer, keyed by Cargo.lock via recipe.json — a source-only commit reuses
# it and rebuilds only the antares-* crates. CI persists the layer with a
# BuildKit gha cache (etsi-matrix.yml), cutting the image build ~10 min → ~3.
FROM rust:1-slim@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c AS chef
WORKDIR /src
# jemalloc ships C sources that configure+make themselves;
# rust:1-slim carries a linker but no make, so tikv-jemalloc-sys' build script
# dies with a bare "No such file or directory". One package, not a toolchain.
RUN apt-get update \
 && apt-get install -y --no-install-recommends make \
 && rm -rf /var/lib/apt/lists/*
# Release binaries embed their dependency list (SBOM) via
# cargo-auditable — `cargo audit bin /antares` can then verify a shipped
# broker against advisories with no source tree at hand. Installed (with chef)
# BEFORE any source COPY so the layer caches across source changes.
RUN cargo install cargo-chef cargo-auditable --locked
# The pinned toolchain must be present from the first cargo invocation —
# otherwise the dep layer compiles with the image's default toolchain and the
# final build recompiles everything under the pinned one, defeating the cache.
COPY rust-toolchain.toml .

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /src/recipe.json recipe.json
# Cook scoped to the shipped package so feature unification matches the real
# build below — a workspace-wide cook can resolve different features and
# quietly recompile deps anyway.
RUN cargo chef cook --release -p antares-broker --recipe-path recipe.json --locked
COPY . .
RUN cargo auditable build --release --locked -p antares-broker \
 && mkdir /data-init

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:adcd20c7b4c988b73cbfbddb26d2eee574571e6d7c9ffea29b3821e0690efb77
COPY --from=build /src/target/release/antares /antares
# jemalloc decay tuning, measured: without a background
# thread, freed pages only purge on ALLOCATION activity — an idle broker
# after a burst parked ~48 MiB of dead pages forever; with it, RSS decays
# back toward live×1.2 within seconds. Both env spellings: tikv-jemalloc
# reads _RJEM_MALLOC_CONF (prefixed build), MALLOC_CONF covers unprefixed.
ENV MALLOC_CONF=background_thread:true,narenas:4,dirty_decay_ms:10000,muzzy_decay_ms:10000 \
    _RJEM_MALLOC_CONF=background_thread:true,narenas:4,dirty_decay_ms:10000,muzzy_decay_ms:10000
# Pre-create /data owned by nonroot (65532): a fresh named volume inherits the
# mountpoint's ownership, so `file` mode can write without running as root —
# distroless has no shell to chown at runtime.
COPY --from=build --chown=65532:65532 /data-init /data
# OCI labels: what the registry shows and what a scanner reads
ARG VERSION=dev
ARG REVISION=unknown
LABEL org.opencontainers.image.title="Antares" \
      org.opencontainers.image.description="NGSI-LD Context Broker (ETSI GS CIM 009 V1.9.1) in Rust" \
      org.opencontainers.image.source="https://github.com/marek-mraz/AntaresBroker" \
      org.opencontainers.image.documentation="https://antares-ngsi-ld-demo.marek-mraz.com/docs/" \
      org.opencontainers.image.licenses="EUPL-1.2" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.revision="$REVISION"
EXPOSE 9090
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s CMD ["/antares", "--health"]
ENTRYPOINT ["/antares"]
