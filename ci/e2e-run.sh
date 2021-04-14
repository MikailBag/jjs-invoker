set -euxo pipefail

skopeo copy dir:e2e-artifacts/invoker docker-daemon:jjs-invoker:latest
skopeo copy dir:e2e-artifacts/shim docker-daemon:jjs-invoker-shim:latest


mkdir e2e-logs
chmod +x e2e-artifacts/test-runner

export DOCKER_BUILDKIT=1
./e2e-artifacts/test-runner --invoker-image=jjs-invoker --shim-image=jjs-invoker-shim --logs=e2e-logs
