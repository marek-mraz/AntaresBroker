# Multi-stage: build on the target arch (arm64/amd64 both fine), run distroless
# (non-root, read-only rootfs friendly — deep-analysis §16.5).
FROM rust:1-slim AS build
WORKDIR /src
# §16.5: release binaries embed their dependency list (SBOM) via
# cargo-auditable — `cargo audit bin /antares` can then verify a shipped
# broker against advisories with no source tree at hand. Installed BEFORE
# the source COPY so the layer caches across source changes.
RUN cargo install cargo-auditable --locked
COPY . .
RUN cargo auditable build --release -p antares-broker \
 && mkdir /data-init

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/antares /antares
# Pre-create /data owned by nonroot (65532): a fresh named volume inherits the
# mountpoint's ownership, so `file` mode can write without running as root —
# distroless has no shell to chown at runtime (§16.5 posture).
COPY --from=build --chown=65532:65532 /data-init /data
EXPOSE 9090
ENTRYPOINT ["/antares"]
