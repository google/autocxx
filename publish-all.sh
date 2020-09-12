#!/bin/bash

set -e

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"

DIRS="$DIR/engine $DIR $DIR/gen/build"

for CRATE in $DIRS; do
  pushd $CRATE
  echo "Dry run: $CRATE"
  cargo publish --dry-run
  echo "Publish: $CRATE"
  cargo publish
  popd
done
