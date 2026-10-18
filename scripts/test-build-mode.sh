#!/bin/zsh
set -euo pipefail
cd "$(dirname "$0")/.."
test_root=$(mktemp -d)
trap 'rm -rf "$test_root"' EXIT
cp build.sh "$test_root/build.sh"
[[ "$(zsh "$test_root/build.sh" --print-mode)" == public ]]
printf 'extended\n' > "$test_root/.take-build-mode"
[[ "$(zsh "$test_root/build.sh" --print-mode)" == public ]]
[[ "$(zsh "$test_root/build.sh" --print-mode --public)" == public ]]
[[ "$(zsh "$test_root/build.sh" --print-mode --local)" == extended ]]
printf 'public\n' > "$test_root/.take-build-mode"
[[ "$(zsh "$test_root/build.sh" --print-mode --local)" == public ]]
printf 'invalid\n' > "$test_root/.take-build-mode"
if zsh "$test_root/build.sh" --print-mode --local; then
  echo 'Invalid local mode was accepted' >&2; exit 1
fi
rm "$test_root/.take-build-mode"
if zsh "$test_root/build.sh" --print-mode --local; then
  echo 'Missing local configuration was accepted' >&2; exit 1
fi
if zsh "$test_root/build.sh" --print-mode --unknown; then
  echo 'Unknown argument was accepted' >&2; exit 1
fi
echo 'Build-mode checks passed'
