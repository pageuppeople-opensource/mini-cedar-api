FROM rust:latest AS build

RUN USER=root cargo new --bin mini_cedar_api
WORKDIR /mini_cedar_api

# copy over your manifests
COPY --link ./Cargo.toml ./Cargo.toml
COPY --link ./cargo.lock ./Cargo.lock

# this build step will cache your dependencies
RUN cargo build --release
RUN rm src/*.rs

COPY --link ./src ./src

RUN rm ./target/release/deps/mini_cedar_api*
RUN cargo build --release

FROM debian:stable-slim

COPY --link --from=build /mini_cedar_api/target/release/mini_cedar_api .
EXPOSE 3000

CMD ["./mini_cedar_api"]