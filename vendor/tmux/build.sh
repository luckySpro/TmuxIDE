#!/usr/bin/env bash
# 编译可随 TmuxIDE 分发的 tmux，输出到 vendor/tmux/out/tmux-<os>-<arch>。
#
#   Linux：在 Alpine (musl) 中完全静态链接，可在任意发行版运行。
#   macOS：libevent、utf8proc 静态链接，只依赖系统自带库（libSystem、libncurses、libresolv）。
#          ARCH=x86_64 或 ARCH=arm64 可在 Apple Silicon 上交叉编译。
#
# 只在构建阶段联网下载并校验源码，终端用户无需安装任何东西。
set -euo pipefail
cd "$(dirname "$0")"
HERE=$PWD
source ./versions.env

OS=$(uname -s)
ARCH=${ARCH:-$(uname -m)}
[ "$ARCH" = "aarch64" ] && [ "$OS" = "Darwin" ] && ARCH=arm64
BUILD=$HERE/build/$OS-$ARCH
PREFIX=$BUILD/prefix
OUT=$HERE/out
JOBS=$( (nproc || sysctl -n hw.ncpu) 2>/dev/null || echo 4)
mkdir -p "$BUILD" "$PREFIX" "$OUT"

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }

fetch() { # url sha256 -> 解压后的目录
  local url=$1 sum=$2 file
  file=$BUILD/$(basename "$url")
  [ -f "$file" ] || curl -fsSL --retry 3 -o "$file" "$url"
  if [ "$(sha256 "$file")" != "$sum" ]; then
    echo "校验失败：$file" >&2
    exit 1
  fi
  tar -xzf "$file" -C "$BUILD"
}

fetch "$TMUX_URL" "$TMUX_SHA256"
TMUX_SRC=$BUILD/tmux-$TMUX_VERSION

if [ "$OS" = "Linux" ]; then
  if ! [ -f /etc/alpine-release ]; then
    echo "Linux 版本需要在 Alpine 容器中构建（musl 完全静态）" >&2
    exit 1
  fi
  apk add --no-cache build-base bison pkgconf libevent-dev libevent-static ncurses-dev ncurses-static linux-headers >/dev/null
  cd "$TMUX_SRC"
  ./configure --enable-static --disable-dependency-tracking --prefix=/usr >/dev/null
  make -j"$JOBS" >/dev/null
  strip tmux
  if ldd ./tmux 2>&1 | grep -q '=>'; then
    echo "tmux 不是静态链接：" >&2
    ldd ./tmux >&2
    exit 1
  fi
  case "$ARCH" in arm64) ARCH=aarch64 ;; esac
  TARGET=$OUT/tmux-linux-$ARCH
elif [ "$OS" = "Darwin" ]; then
  export MACOSX_DEPLOYMENT_TARGET=11.0
  export CC="clang -arch $ARCH"
  export CFLAGS="-O2 -mmacosx-version-min=11.0"
  HOST_FLAG=""
  if [ "$ARCH" != "$(uname -m)" ]; then
    HOST_FLAG="--host=${ARCH/arm64/aarch64}-apple-darwin"
  fi
  # 只让 pkg-config 看到我们自己编译的库，避免链接到 Homebrew 的动态库
  export PKG_CONFIG_LIBDIR=$PREFIX/lib/pkgconfig
  export PKG_CONFIG_PATH=

  # libevent（静态）
  fetch "$LIBEVENT_URL" "$LIBEVENT_SHA256"
  (cd "$BUILD/libevent-$LIBEVENT_VERSION" &&
    ./configure --prefix="$PREFIX" $HOST_FLAG --disable-shared --enable-static --disable-openssl \
      --disable-samples --disable-libevent-regress --disable-debug-mode --disable-dependency-tracking >/dev/null &&
    make -j"$JOBS" >/dev/null && make install >/dev/null)

  # utf8proc（静态）：macOS 自带的 wcwidth 过旧，tmux 推荐使用 utf8proc
  if ! [ -d "$BUILD/utf8proc" ]; then
    git clone -q --depth 1 --branch "$UTF8PROC_TAG" "$UTF8PROC_REPO" "$BUILD/utf8proc"
  fi
  if [ "$(git -C "$BUILD/utf8proc" rev-parse HEAD)" != "$UTF8PROC_COMMIT" ]; then
    echo "utf8proc 提交哈希不匹配" >&2
    exit 1
  fi
  (cd "$BUILD/utf8proc" && make clean >/dev/null 2>&1 || true)
  (cd "$BUILD/utf8proc" && make CC="$CC" CFLAGS="$CFLAGS" libutf8proc.a >/dev/null)
  cp "$BUILD/utf8proc/libutf8proc.a" "$PREFIX/lib/"
  cp "$BUILD/utf8proc/utf8proc.h" "$PREFIX/include/"

  cd "$TMUX_SRC"
  ./configure $HOST_FLAG --enable-utf8proc --disable-dependency-tracking --prefix=/usr/local \
    CPPFLAGS="-I$PREFIX/include -DUTF8PROC_STATIC" LDFLAGS="-L$PREFIX/lib" \
    LIBUTF8PROC_CFLAGS="-I$PREFIX/include -DUTF8PROC_STATIC" LIBUTF8PROC_LIBS="$PREFIX/lib/libutf8proc.a" \
    LIBEVENT_CORE_CFLAGS="-I$PREFIX/include" LIBEVENT_CORE_LIBS="$PREFIX/lib/libevent_core.a" \
    LIBEVENT_CFLAGS="-I$PREFIX/include" LIBEVENT_LIBS="$PREFIX/lib/libevent.a" >/dev/null
  make -j"$JOBS" >/dev/null
  strip tmux
  # 确认只链接系统库
  if otool -L tmux | tail -n +2 | grep -vE '^\s+/(usr/lib|System/Library)/'; then
    echo "tmux 链接了非系统库" >&2
    exit 1
  fi
  TARGET=$OUT/tmux-macos-$ARCH
else
  echo "不支持的系统：$OS" >&2
  exit 1
fi

cp "$TMUX_SRC/tmux" "$TARGET"
mkdir -p "$OUT/licenses"
cp "$TMUX_SRC/COPYING" "$OUT/licenses/tmux-COPYING"
echo "$TMUX_VERSION" > "$OUT/VERSION"
echo "已生成 $TARGET"
file "$TARGET" 2>/dev/null || true
