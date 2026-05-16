FROM rust:1.78-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /usr/src/silex-ingress
COPY . .
RUN cargo build --release

FROM scratch
COPY --from=builder /usr/src/silex-ingress/target/release/silex-ingress /silex-ingress
ENTRYPOINT ["/silex-ingress"]