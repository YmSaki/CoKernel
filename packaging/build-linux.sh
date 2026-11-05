#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
mkdir -p target/release-inputs target/release-stage
inputs="$root/target/release-inputs"
stage="$root/target/release-stage"
fetch() {
  local url="$1" hash="$2" file="$3"
  if [[ ! -f "$file" ]]; then curl --proto '=https' --tlsv1.2 -fLSs --retry 3 "$url" -o "$file"; fi
  printf '%s  %s\n' "$hash" "$file" | sha256sum --check --status
}
fetch https://cdimage.ubuntu.com/ubuntu-base/releases/24.04/release/ubuntu-base-24.04.5-base-amd64.tar.gz e77b6f10c2590cef872b33ee9f635a0e3fd1f57fb074c0e52b5c7f56147a0c86 "$inputs/base.tar.gz"
fetch https://github.com/astral-sh/uv/releases/download/0.12.12/uv-x86_64-unknown-linux-gnu.tar.gz ab9b309d4586403f024e100abaceb396616e178a553e2500c36087d180f09509 "$inputs/uv.tar.gz"
fetch https://github.com/openai/tunnel-client/releases/download/v0.0.14/tunnel-client-v0.0.14-linux-amd64.zip 15bd17e805cad39d412199115bb9e10a978dd35258a114cdf25dd2ae6681c7d3 "$inputs/tunnel.zip"
fetch https://archive.ubuntu.com/ubuntu/pool/main/c/ca-certificates/ca-certificates_20260816ubuntu0.24.04.1_all.deb 8a0a754aaafed72cc8efb8c1731808133284a84fbfc76a4fcc73fce9e01b4187 "$inputs/ca.deb"
tar -xzf "$inputs/uv.tar.gz" -C "$inputs"
unzip -oq "$inputs/tunnel.zip" -d "$inputs/tunnel"
dpkg-deb -x "$inputs/ca.deb" "$inputs/ca"
LC_ALL=C cat "$inputs"/ca/usr/share/ca-certificates/mozilla/*.crt > "$inputs/ca-certificates.crt"
uv="$inputs/uv-x86_64-unknown-linux-gnu/uv"
export UV_PYTHON_INSTALL_DIR="$inputs/python"
"$uv" python install 3.12.14
python_bin="$("$uv" python find --managed-python 3.12.14)"
python_root="$(dirname "$(dirname "$python_bin")")"
tunnel="$(find "$inputs/tunnel" -type f -name tunnel-client -print -quit)"
test -n "$tunnel"
chmod +x "$tunnel"
cargo build --locked --release -p cokernel-runtime -p cokernel-mcp -p cokernel-bootstrap
python3 scripts/v1/build_validation_payload.py "$inputs/payload.tar" --profile release --python "$python_root" --uv "$uv" --tunnel "$tunnel" --ca-bundle "$inputs/ca-certificates.crt"
target/release/cokernel-bootstrap image "$inputs/base.tar.gz" "$inputs/payload.tar" "$stage/runtime.tar"
cp "$inputs/tunnel"/*licenses* "$inputs/tunnel"/*.spdx.json "$stage/"
cp "$inputs/tunnel/LICENSE" "$stage/tunnel-LICENSE"
cp "$inputs/tunnel/NOTICE" "$stage/tunnel-NOTICE"
# Explicit source identity is shipped alongside the immutable image receipt.
git rev-parse HEAD > "$stage/source-commit.txt"
