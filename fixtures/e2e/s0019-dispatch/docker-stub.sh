#!/bin/sh
# S0019 p6-4 — a stub `docker` inside the TARGET, standing in for the node container. It answers the
# probe's inspect/ps/exec and RECORDS a restart (so the test can assert the executor really ran).
LOG=/var/lib/ouro/docker-calls.log
echo "$*" >> "$LOG"
case "$1 $2" in
  "ps --filter") echo "nodecid" ;;
  "ps --no-trunc") echo "nodecid cardano-node run" ;;
  "ps --format") echo "nodecid cardano-node run" ;;
  "inspect --format")
    case "$3" in
      "{{.Image}}") echo "sha256:beddispatch" ;;
      "{{.Created}}") echo "2026-07-15T10:00:00Z" ;;
      "{{json .Config.Entrypoint}}") echo '["cardano-node"]' ;;
      "{{json .Args}}") echo '["run"]' ;;
      "{{range .Mounts}}{{.Source}};{{end}}") echo "/srv/data;" ;;
      "{{.HostConfig.RestartPolicy.Name}}") echo "unless-stopped" ;;
      "{{.State.StartedAt}}") cat /var/lib/ouro/started 2>/dev/null || echo "t0" ;;
      *) echo "" ;;
    esac ;;
  "exec nodecid")
    # docker exec nodecid sh -c '...'
    shift 2
    if echo "$*" | grep -q kes.skey; then echo true
    elif echo "$*" | grep -q node.cert; then echo "opcerthash  /x"
    elif echo "$*" | grep -q sha256sum; then echo "cfghash  /x"
    else echo "0" ; fi ;;
  "restart nodecid"|"restart "*)
    # record a real restart: advance the StartedAt marker
    date +%s%N > /var/lib/ouro/started ;;
esac
exit 0
