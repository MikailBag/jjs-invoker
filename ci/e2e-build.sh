set -euxo pipefail

curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo apt-key add -
sudo add-apt-repository \
  "deb [arch=amd64] https://download.docker.com/linux/ubuntu \
  $(lsb_release -cs) \
  stable"
sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io

bash ./scripts/build-all.sh
export RUSTC_BOOTSTRAP=1
cargo build -p test-runner -Zunstable-options --out-dir ./out

mkdir e2e-artifacts
cp ./out/test-runner e2e-artifacts/test-runner
skopeo copy docker-daemon:jjs-invoker:latest dir:e2e-artifacts/invoker
skopeo copy docker-daemon:jjs-invoker-shim:latest dir:e2e-artifacts/shim
skopeo copy docker-daemon:jjs-invoker-strace-debugger:latest dir:e2e-artifacts/debugger
