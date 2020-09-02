set -euo pipefail
# TODO skip cgroups v1
mkdir -p /sys/fs/cgroup/jjs/
echo "+pids +memory +cpu" | tee "/sys/fs/cgroup/cgroup.subtree_control"
echo "+pids +memory +cpu" | tee "/sys/fs/cgroup/jjs/cgroup.subtree_control"
exec /bin/invoker "$@"
