#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <tag-or-version> <artifacts-dir>" >&2
  exit 1
fi

tag_or_version="$1"
artifacts_dir="$2"

version="${tag_or_version#v}"
formula_name="ani-nexus-tui"
binary_name="ani-nexus"
target_archive_name="${formula_name}-x86_64-unknown-linux-gnu.tar.xz"
bottle_name="${formula_name}--${version}.x86_64_linux.bottle.tar.gz"

archive_path="$(find "$artifacts_dir" -maxdepth 1 -type f -name "$target_archive_name" | head -n1 || true)"
if [[ -z "$archive_path" ]]; then
  echo "Linux dist archive not found: $target_archive_name" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

extract_dir="$tmp_dir/extract"
stage_root="$tmp_dir/stage"
stage_prefix="$stage_root/$formula_name/$version"

mkdir -p "$extract_dir" "$stage_prefix/bin" "$stage_prefix/.brew"
tar -xJf "$archive_path" -C "$extract_dir"

payload_dir="$(find "$extract_dir" -mindepth 1 -maxdepth 1 -type d | head -n1 || true)"
if [[ -z "$payload_dir" ]]; then
  echo "Unable to locate unpacked payload directory" >&2
  exit 1
fi

if [[ ! -f "$payload_dir/$binary_name" ]]; then
  echo "Expected binary not found in payload: $binary_name" >&2
  exit 1
fi

cp "$payload_dir/$binary_name" "$stage_prefix/bin/$binary_name"
ln -s "$binary_name" "$stage_prefix/bin/$formula_name"

if [[ -f "$payload_dir/LICENSE" ]]; then
  cp "$payload_dir/LICENSE" "$stage_prefix/LICENSE"
fi
if [[ -f "$payload_dir/README.md" ]]; then
  cp "$payload_dir/README.md" "$stage_prefix/README.md"
fi

cat > "$stage_prefix/.brew/$formula_name.rb" <<EOF
class AniNexusTui < Formula
  desc "Blazing-fast TUI for Anime"
  homepage "https://github.com/OsamuDazai666/ani-nexus-tui"
  version "${version}"
  license "CC-BY-NC-SA-4.0"
end
EOF

output_path="$artifacts_dir/$bottle_name"
tar -C "$stage_root" -czf "$output_path" "$formula_name"
echo "Created Homebrew bottle: $output_path"
