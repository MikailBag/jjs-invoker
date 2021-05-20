set -euxo pipefail

skopeo copy dir:e2e-artifacts/invoker docker-daemon:jjs-invoker:latest
skopeo copy dir:e2e-artifacts/shim docker-daemon:jjs-invoker-shim:latest
skopeo copy dir:e2e-artifacts/debugger docker-daemon:jjs-invoker-strace-debugger:latest


mkdir e2e-logs
chmod +x e2e-artifacts/test-runner

export DOCKER_BUILDKIT=1
    ./e2e-artifacts/test-runner \
    --invoker-image=jjs-invoker \
    --shim-image=jjs-invoker-shim \
    --strace-debug-image=jjs-invoker-strace-debugger \
    --logs=e2e-logs
