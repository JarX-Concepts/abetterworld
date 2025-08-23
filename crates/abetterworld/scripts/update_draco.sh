#!/bin/bash
set -euxo pipefail

# Define the version or branch you want (use a tag for reproducibility)
DRACO_REPO=https://github.com/google/draco.git
DRACO_DIR=draco
DRACO_TAG=main  # change to e.g. 1.5.6 for a fixed release

# Remove existing
rm -rf "$DRACO_DIR"

# Clone fresh copy of Draco
git clone --depth 1 --branch "$DRACO_TAG" "$DRACO_REPO" "$DRACO_DIR"

# Optionally prune unnecessary files to shrink package
pushd "$DRACO_DIR"
rm -rf \
  .git* \
  javascript \
  python \
  tools \
  docs \
  test \
  examples \
  third_party \
  testdata \
  maya \
  unity
popd