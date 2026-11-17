#!/bin/bash
# Invoked with an empty PATH in the release container.
set -eu
input=${1:?qualified release directory}
scratch=${2:?scratch directory}

"$input/vnc" exec "$input/hello.vn" --out "$scratch/hello" -- --json
"$input/vnc" build "$input/project/applications/settings/main.vn" \
    --adapter-pack "$input/settings.adapter" --out "$scratch/settings"

cli="$scratch/settings"
store="$scratch/settings.json"
"$cli" --entry set -- "$store" theme dark
value=$("$cli" --entry get -- "$store" theme)
test "$value" = "$(printf 'dark\ndone')"

: > "$scratch/address"
"$cli" --entry serve -- "$store" 0 1 10000 > "$scratch/address" &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT
for ((i=0; i<150; i++)); do
    if read -r address < "$scratch/address"; then break; fi
    kill -0 "$server"
    /bin/sleep 0.1
done
[[ "${address:-}" == http://127.0.0.1:* ]] || exit 1
authority=${address#http://}
authority=${authority%/}
exec 3<>"/dev/tcp/127.0.0.1/${authority##*:}"
printf 'GET /settings HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n\r\n' "$authority" >&3
response=$(/bin/cat <&3)
exec 3>&-
[[ "$response" == 'HTTP/1.1 200 '* ]] || exit 1
body=${response#*$'\r\n\r\n'}
[[ "${body//[[:space:]]/}" == '{"theme":"dark"}' ]] || exit 1
wait "$server"
trap - EXIT
