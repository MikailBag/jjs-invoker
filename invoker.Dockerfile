FROM ubuntu:focal
COPY out/invoker /bin/invoker
COPY /scripts/docker-entrypoint.sh /entry.sh
ENTRYPOINT [ "/bin/bash", "/entry.sh" ]
VOLUME ["/var/judges"]
EXPOSE 8000
CMD [ "--work-dir", "/var/judges", "--listen-address", "tcp://0.0.0.0:8000" ]
