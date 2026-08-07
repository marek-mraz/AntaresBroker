# Multi-stage: build on the target arch (arm64/amd64 both fine), run distroless
# (non-root, read-only rootfs friendly — deep-analysis §16.5).
#
# cargo-chef splits the build so the ~400 dependency crates compile in their
# own layer, keyed by Cargo.lock via recipe.json — a source-only commit reuses
# it and rebuilds just the antares-* crates. CI persists the layer with a
# BuildKit gha cache (etsi-matrix.yml), cutting the image build ~10 min → ~3.
FROM rust:1-slim AS chef
WORKDIR /src
# jemalloc (§2.1 allocator) ships C sources that configure+make themselves;
# rust:1-slim carries a linker but no make, so tikv-jemalloc-sys' build script
# dies with a bare "No such file or directory". One package, not a toolchain.
RUN apt-get update \
 && apt-get install -y --no-install-recommends make \
 && rm -rf /var/lib/apt/lists/*
# §16.5: release binaries embed their dependency list (SBOM) via
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
RUN cargo chef cook --release -p antares-broker --recipe-path recipe.json
COPY . .
RUN cargo auditable build --release -p antares-broker \
 && mkdir /data-init

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/antares /antares
# §6.1 jemalloc decay tuning, measured (2026-08-07): without a background
# thread, freed pages only purge on ALLOCATION activity — an idle broker
# after a burst parked ~48 MiB of dead pages forever; with it, RSS decays
# back toward live×1.2 within seconds. Both env spellings: tikv-jemalloc
# reads _RJEM_MALLOC_CONF (prefixed build), MALLOC_CONF covers unprefixed.
ENV MALLOC_CONF=background_thread:true,dirty_decay_ms:10000,muzzy_decay_ms:10000 \
    _RJEM_MALLOC_CONF=background_thread:true,dirty_decay_ms:10000,muzzy_decay_ms:10000
# Pre-create /data owned by nonroot (65532): a fresh named volume inherits the
# mountpoint's ownership, so `file` mode can write without running as root —
# distroless has no shell to chown at runtime (§16.5 posture).
COPY --from=build --chown=65532:65532 /data-init /data
EXPOSE 9090
ENTRYPOINT ["/antares"]
