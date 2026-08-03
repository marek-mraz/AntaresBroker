# Multi-stage: build on the target arch (arm64/amd64 both fine), run distroless
# (non-root, read-only rootfs friendly — deep-analysis §16.5).
FROM rust:1-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p antares-broker

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/antares /antares
EXPOSE 9090
ENTRYPOINT ["/antares"]
