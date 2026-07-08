#!/usr/bin/env bash
# S0015 E2E cleanup — remove ONLY the artifacts this repo's tests created, and leave any
# pre-existing user containers/images untouched. Safe to run any time.
set -uo pipefail

echo "[clean] S0015 containers (ouro-e2e-*, devnet-node) — NOT other cardano-node containers"
docker ps -aq --filter 'name=ouro-e2e' | xargs -r docker rm -f >/dev/null 2>&1 || true
docker rm -f devnet-node >/dev/null 2>&1 || true

echo "[clean] S0015 images this repo builds/pulls (preserves anything else)"
for img in \
  ouro-e2e-base:local \
  ghcr.io/blinklabs-io/cardano-node:10.5.4 \
  ghcr.io/intersectmbo/cardano-node:10.5.4 \
  hello-world:latest; do
  docker image rm "$img" >/dev/null 2>&1 && echo "  removed $img" || true
done

echo "[clean] dangling images + build cache from our compiles"
docker image prune -f >/dev/null 2>&1 || true
docker builder prune -f >/dev/null 2>&1 || true   # reclaims the rust-in-docker ouro build cache

echo "[clean] host temp artifacts"
rm -rf /tmp/devnet /tmp/ouro-* 2>/dev/null || true

echo "[clean] done. Preserved (not ours): any cardano-node:10.2.1-1 / non-ouro-e2e containers,"
echo "        and shared bases (debian:12-slim, rust:*). Remove those manually if desired."
