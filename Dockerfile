FROM rust:1.49 AS builder
WORKDIR /app
COPY ./src/invoker/Cargo.toml ./
# This way dependencies will be cached separately from invoker source,
# so code changes will not invalidate the whole build.
RUN mkdir ./src && \
    echo 'fn main(){}' > ./src/main.rs && \
    cargo build --release && \
    rm -r ./src/
COPY ./src/invoker ./
RUN touch src/main.rs && \
    cargo build --release

FROM debian:stable-slim
RUN apt-get update && apt-get install -y curl
COPY --from=builder /app/target/release/invoker /bin/invoker
COPY /scripts/docker-entrypoint.sh /entry.sh
ENTRYPOINT [ "/bin/bash", "/entry.sh" ]
VOLUME ["/var/judges"]
EXPOSE 8000
HEALTHCHECK --start-period=1ms --interval=2s CMD curl http://localhost:8000/ready || exit 1
CMD [ "--work-dir", "/var/judges" ]
