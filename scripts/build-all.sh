set -euxo pipefail

docker build . --target invoker "--build-arg=EXTRA_ARGS=$@" --tag jjs-invoker
docker build . --target shim "--build-arg=EXTRA_ARGS=$@" --tag jjs-invoker-shim
docker build . --target strace-debug "--build-arg=EXTRA_ARGS=$@" --tag jjs-invoker-strace-debugger