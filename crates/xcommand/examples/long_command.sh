#!/usr/bin/env bash
set -Eeuo pipefail

for i in {1..5}; do
  echo "Sleeping ${i}"
  sleep 1
done

echo "Done"
