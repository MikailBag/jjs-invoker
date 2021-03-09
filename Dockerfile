FROM debian:stable-slim
RUN apt-get update && apt-get install -y curl
COPY out/invoker /bin/invoker
COPY /scripts/docker-entrypoint.sh /entry.sh
ENTRYPOINT [ "/bin/bash", "/entry.sh" ]
VOLUME ["/var/judges"]
EXPOSE 8000
HEALTHCHECK --start-period=1ms --interval=2s CMD curl http://localhost:8000/ready || exit 1
CMD [ "--work-dir", "/var/judges", "--listen-address", "tcp://0.0.0.0:8000" ]
