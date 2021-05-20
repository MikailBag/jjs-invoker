FROM lukemathwalker/cargo-chef as build-plan
WORKDIR /app
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM lukemathwalker/cargo-chef as cache
WORKDIR /app
COPY --from=build-plan /app/recipe.json recipe.json
ARG EXTRA_ARGS=""
RUN cargo chef cook ${EXTRA_ARGS} --recipe-path recipe.json

FROM rust as build
WORKDIR /app
COPY . .
COPY --from=cache /app/target target
COPY --from=cache $CARGO_HOME $CARGO_HOME
ARG EXTRA_ARGS=""
ENV RUSTC_BOOTSTRAP=1
RUN cargo build ${EXTRA_ARGS} -Zunstable-options --out-dir ./out

FROM gcr.io/distroless/cc as invoker
COPY --from=build /app/out/invoker /usr/local/bin/invoker
ENTRYPOINT [ "/usr/local/bin/invoker" ]
VOLUME ["/var/judges"]
EXPOSE 8000
CMD [ "--work-dir", "/var/judges", "--listen-address", "tcp://0.0.0.0:8000" ]

FROM gcr.io/distroless/cc as shim
COPY --from=build /app/out/shim /usr/local/bin/shim
ENTRYPOINT [ "/usr/local/bin/shim" ]
EXPOSE 8001
CMD [ "--port", "8001" ]

FROM ubuntu:focal as strace-debug
RUN apt update && apt install -y strace
COPY --from=build /app/out/strace-debugger /usr/local/bin/debugger
ENTRYPOINT [ "/usr/local/bin/debugger" ]
EXPOSE 8000

FROM scratch
RUN you have forgotten to specify build target
