#!/usr/bin/env bash
# Optional: clone upstream reference repos on a machine with internet access.
set -euo pipefail
mkdir -p references
if [[ ! -e references/sparkplayer ]]; then
  git clone --depth 1 https://github.com/dividebysandwich/sparkplayer.git references/sparkplayer
fi
if [[ ! -e references/vlc ]]; then
  git clone --depth 1 --filter=blob:none https://code.videolan.org/videolan/vlc.git references/vlc
fi
printf '%s\n' 'Upstream sources are in references/ (not needed to run TermVLC).'
