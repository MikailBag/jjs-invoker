set -euxo pipefail

docker build . --target invoker "--build-arg=EXTRA_ARGS=$@" --tag invoker
docker build . --target shim "--build-arg=EXTRA_ARGS=$@" --tag invoker-shim
docker build . --target strace-debug "--build-arg=EXTRA_ARGS=$@" --tag invoker-strace-debugger
