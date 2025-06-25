FROM rust:slim-bookworm AS build
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        libclang-dev
WORKDIR /src
COPY . .
RUN cargo install --path .

FROM rust:slim-bookworm
COPY --from=build /src/target/release/rdmx /usr/bin/rdmx
CMD ["rdmx"]
