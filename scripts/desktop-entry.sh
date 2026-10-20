#!/bin/sh
# Usage: scripts/desktop-entry.sh <path of the codescape binary> < packaging/linux/codescape.desktop
set -eu
EXEC=$(printf '%s' "$1" | sed -e 's/[\\"`$]/\\&/g' -e 's/\\/\\\\/g' -e 's/%/%%/g') awk '/^Exec=/ { print "Exec=\"" ENVIRON["EXEC"] "\" %f"; next } { print }'
