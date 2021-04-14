FROM debian:stable-slim
RUN apt update && apt install -y curl
COPY out/shim /bin/shim
ENTRYPOINT [ "/bin/shim" ]
EXPOSE 8001
HEALTHCHECK --start-period=100ms --interval=2s CMD curl http://localhost:8001/ready || exit 1
CMD [ "--port", "8001" ]
