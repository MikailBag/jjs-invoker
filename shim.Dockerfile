FROM gcr.io/distroless/base
COPY out/shim /bin/shim
ENTRYPOINT [ "/bin/shim" ]
EXPOSE 8001
CMD [ "--port", "8001" ]
