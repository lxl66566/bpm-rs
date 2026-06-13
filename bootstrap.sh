#!/bin/sh
set -euxo pipefail

REPO="lxl66566/bpm-rs"
VERSION="${BPM_VERSION:-latest}"

detect_target() {
	OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
	ARCH="$(uname -m)"

	case "$ARCH" in
	x86_64 | amd64) ARCH="x86_64" ;;
	aarch64 | arm64) ARCH="aarch64" ;;
	*)
		echo "Unsupported architecture: $ARCH" >&2
		exit 1
		;;
	esac

	case "$OS" in
	linux)
		LIBC="gnu"
		[ -f /etc/alpine-release ] && LIBC="musl"
		echo "$ARCH-unknown-linux-$LIBC"
		;;
	darwin)
		echo "$ARCH-apple-darwin"
		;;
	*)
		echo "Unsupported OS: $OS" >&2
		exit 1
		;;
	esac
}

TARGET="${BPM_TARGET:-$(detect_target)}"

if [ "$VERSION" = "latest" ]; then
	URL="https://github.com/$REPO/releases/latest/download/bin-package-manager-$TARGET.tar.gz"
else
	URL="https://github.com/$REPO/releases/download/$VERSION/bin-package-manager-$TARGET.tar.gz"
fi

TMP="$(mktemp -d)"
trap 'rm -rf -- "${TMP:-}"' EXIT

echo "Downloading bpm for $TARGET..."
curl -fsSL "$URL" -o "$TMP/pkg.tar.gz"

echo "Extracting..."
mkdir -p "$TMP/extract"
tar xzf "$TMP/pkg.tar.gz" -C "$TMP/extract"

BPM="$(find "$TMP/extract" -type f -name 'bpm' 2>/dev/null | head -n1)"
if [ -z "$BPM" ]; then
	echo "Error: bpm binary not found in archive." >&2
	exit 1
fi
chmod +x "$BPM"

if [ "$(id -u)" -ne 0 ]; then
	echo "Warning: bpm requires root privileges on Unix. Re-run with sudo if installation fails." >&2
fi

echo "Installing bpm..."
"$BPM" install --local "$TMP/pkg.tar.gz" bpm

echo "bpm installed! Run 'bpm --help' to get started."
