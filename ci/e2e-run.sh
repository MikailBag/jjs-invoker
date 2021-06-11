set -euxo pipefail

skopeo copy dir:e2e-artifacts/invoker docker-daemon:invoker:latest
skopeo copy dir:e2e-artifacts/shim docker-daemon:invoker-shim:latest
skopeo copy dir:e2e-artifacts/debugger docker-daemon:invoker-strace-debugger:latest


mkdir e2e-logs
chmod +x e2e-artifacts/test-runner

export DOCKER_BUILDKIT=1
    ./e2e-artifacts/test-runner \
    --invoker-image=invoker \
    --shim-image=invoker-shim \
    --strace-debug-image=invoker-strace-debugger \
    --logs=e2e-logs
