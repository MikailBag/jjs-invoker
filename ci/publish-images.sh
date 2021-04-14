set -euxo pipefail

# GENERATED FILE DO NOT EDIT
if [ "$GITHUB_REF" = "refs/heads/master" ]
then
  TAG="latest"
elif [ "$GITHUB_REF" = "refs/heads/trying" ]
then
  TAG="dev"
else
  echo "unknown GITHUB_REF: $GITHUB_REF"
  exit 1
fi
echo ${{ secrets.GHCR_TOKEN }} | docker login ghcr.io -u $GITHUB_ACTOR --password-stdin
docker tag jjs-invoker ghcr.io/jjs-dev/jjs-invoker:$TAG
docker push ghcr.io/jjs-dev/jjs-invoker:$TAG