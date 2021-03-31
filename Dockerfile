FROM alpine:3
RUN apk add --no-cache curl
COPY out/invoker /bin/invoker
COPY /scripts/docker-entrypoint.sh /entry.sh
ENTRYPOINT [ "/bin/sh", "/entry.sh" ]
VOLUME ["/var/judges"]
EXPOSE 8000
HEALTHCHECK --start-period=1ms --interval=2s CMD curl http://localhost:8000/ready || exit 1
CMD [ "--work-dir", "/var/judges", "--listen-address", "tcp://0.0.0.0:8000" ]
