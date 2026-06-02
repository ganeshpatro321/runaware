#!/usr/bin/env sh
set -eu

repo="${RUNAWARE_REPO:-ganeshpatro321/runaware}"
version="${RUNAWARE_VERSION:-latest}"
install_dir="${RUNAWARE_INSTALL_DIR:-$HOME/.local/bin}"

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Darwin)
    case "$arch" in
      arm64|aarch64) target="aarch64-apple-darwin" ;;
      x86_64) target="x86_64-apple-darwin" ;;
      *) echo "unsupported macOS architecture: $arch" >&2; exit 1 ;;
    esac
    ;;
  Linux)
    case "$arch" in
      x86_64|amd64) target="x86_64-unknown-linux-gnu" ;;
      *) echo "unsupported Linux architecture: $arch" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "unsupported OS: $os" >&2
    exit 1
    ;;
esac

asset="runaware-${target}.tar.gz"
if [ "$version" = "latest" ]; then
  url="https://github.com/${repo}/releases/latest/download/${asset}"
else
  url="https://github.com/${repo}/releases/download/${version}/${asset}"
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

mkdir -p "$install_dir"
echo "Downloading $url"
curl -fsSL "$url" -o "$tmp_dir/$asset"
tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"
install -m 755 "$tmp_dir/runaware-${target}/runaware" "$install_dir/runaware"

echo "Installed runaware to $install_dir/runaware"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) echo "Add $install_dir to your PATH if runaware is not found." ;;
esac
