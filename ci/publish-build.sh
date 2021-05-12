set -euxo pipefail

export DOCKER_BUILDKIT=1
bash ./scripts/build-all.sh --release
