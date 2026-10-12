#!/bin/sh
# 下载并校验内置 cloudflared（Cloudflare 隧道客户端，Apache-2.0）
# 用法：OS=linux|macos ARCH=x86_64|aarch64|arm64 vendor/cloudflared/fetch.sh
# 输出：vendor/cloudflared/out/cloudflared-$OS-$ARCH、VERSION、licenses/
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/versions.env"
: "${OS:?需要 OS}" "${ARCH:?需要 ARCH}"

case "$OS-$ARCH" in
  linux-x86_64) ASSET=cloudflared-linux-amd64 ;;
  linux-aarch64) ASSET=cloudflared-linux-arm64 ;;
  macos-arm64) ASSET=cloudflared-darwin-arm64.tgz ;;
  macos-x86_64) ASSET=cloudflared-darwin-amd64.tgz ;;
  *) echo "不支持的平台 $OS-$ARCH" >&2; exit 1 ;;
esac
eval "WANT=\${CLOUDFLARED_SHA256_${OS}_${ARCH}}"

OUT="$HERE/out"
TMP="$HERE/build"
rm -rf "$TMP"
mkdir -p "$OUT/licenses" "$TMP"
curl -fsSL --retry 4 -o "$TMP/$ASSET" \
  "https://github.com/cloudflare/cloudflared/releases/download/$CLOUDFLARED_VERSION/$ASSET"

if command -v sha256sum >/dev/null; then GOT=$(sha256sum "$TMP/$ASSET" | cut -d' ' -f1)
else GOT=$(shasum -a 256 "$TMP/$ASSET" | cut -d' ' -f1); fi
if [ "$GOT" != "$WANT" ]; then
  echo "cloudflared 校验失败：期望 $WANT，实际 $GOT" >&2
  exit 1
fi

BIN="$OUT/cloudflared-$OS-$ARCH"
case "$ASSET" in
  *.tgz) tar -xzf "$TMP/$ASSET" -C "$TMP" cloudflared && mv "$TMP/cloudflared" "$BIN" ;;
  *) mv "$TMP/$ASSET" "$BIN" ;;
esac
chmod 755 "$BIN"
echo "$CLOUDFLARED_VERSION" > "$OUT/VERSION"
cp "$HERE/LICENSE" "$OUT/licenses/cloudflared-LICENSE.txt"
rm -rf "$TMP"
echo "cloudflared $CLOUDFLARED_VERSION -> $BIN"
