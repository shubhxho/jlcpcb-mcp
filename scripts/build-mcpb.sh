#!/usr/bin/env bash
# Build an MCP Bundle (.mcpb) for the HOST platform from mcpb/manifest.json.
#
# A .mcpb ships a prebuilt binary, so each bundle only runs on the OS/arch it
# was built on. Run this on each target platform (or in a CI matrix) and
# attach the resulting files to a GitHub release, e.g.
#   dist/jlcpcb-mcp-0.1.0-linux-x86_64.mcpb
# Requires: cargo, node/npx (for @anthropic-ai/mcpb).
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
exe=jlcpcb-mcp
case "$os" in mingw*|msys*|cygwin*) os=windows; exe=jlcpcb-mcp.exe ;; esac

cargo build --release --locked

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/server"
cp mcpb/manifest.json README.md LICENSE "$stage/"
cp "target/release/$exe" "$stage/server/"

# Restrict the bundle to the platform it actually contains.
case "$os" in
  darwin) plat=darwin ;; windows) plat=win32 ;; *) plat=linux ;;
esac
node -e '
const fs=require("fs");const p=process.argv[1];const m=JSON.parse(fs.readFileSync(p));
m.compatibility.platforms=[process.argv[2]];fs.writeFileSync(p,JSON.stringify(m,null,2)+"\n");
' "$stage/manifest.json" "$plat"

npx -y @anthropic-ai/mcpb validate "$stage/manifest.json"
mkdir -p dist
out="dist/jlcpcb-mcp-${version}-${os}-${arch}.mcpb"
npx -y @anthropic-ai/mcpb pack "$stage" "$out"
echo "built $out"
