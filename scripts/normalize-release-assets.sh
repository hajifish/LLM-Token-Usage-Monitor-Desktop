#!/usr/bin/env bash
# 统一发布产物命名，并同步改写 latest.json 里的下载地址。
#
# 用法: scripts/normalize-release-assets.sh <tag> <version>
#   例: scripts/normalize-release-assets.sh v0.3.0 0.3.0
#
# 为什么需要：Tauri 按 productName 生成产物名，会把空格换成点，得到
#   Token.Usage_0.3.0_aarch64.dmg
# 这里统一成与发布标题「Token Usage 0.3.0」对得上的形式：
#   TokenUsage-0.3.0-aarch64.dmg
#
# 只重命名文件，不动 .sig 的内容，所以更新包的签名校验不受影响；
# latest.json 里的 url 同步替换，更新链路保持可用。
set -euo pipefail

TAG="${1:?用法: normalize-release-assets.sh <tag> <version>}"
VERSION="${2:?用法: normalize-release-assets.sh <tag> <version>}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

gh release download "$TAG" --dir "$WORK" --clobber

# 旧名 -> 新名。Tauri 改了产物命名时这里会直接报错退出，
# 而不是把没认出来的文件悄悄漏掉。
target_name() {
  case "$1" in
    "latest.json")                                echo "latest.json" ;;
    "Token.Usage_${VERSION}_aarch64.dmg")         echo "TokenUsage-${VERSION}-aarch64.dmg" ;;
    "Token.Usage_aarch64.app.tar.gz")             echo "TokenUsage-${VERSION}-aarch64.app.tar.gz" ;;
    "Token.Usage_aarch64.app.tar.gz.sig")         echo "TokenUsage-${VERSION}-aarch64.app.tar.gz.sig" ;;
    "Token.Usage_${VERSION}_x64-setup.exe")       echo "TokenUsage-${VERSION}-x64-setup.exe" ;;
    "Token.Usage_${VERSION}_x64-setup.exe.sig")   echo "TokenUsage-${VERSION}-x64-setup.exe.sig" ;;
    "Token.Usage_${VERSION}_x64_en-US.msi")       echo "TokenUsage-${VERSION}-x64.msi" ;;
    "Token.Usage_${VERSION}_x64_en-US.msi.sig")   echo "TokenUsage-${VERSION}-x64.msi.sig" ;;
    *) return 1 ;;
  esac
}

renamed=()
for path in "$WORK"/*; do
  base="$(basename "$path")"
  if ! new="$(target_name "$base")"; then
    echo "::error::未预期的发布产物: $base（若是 Tauri 改了命名，请更新 scripts/normalize-release-assets.sh）"
    exit 1
  fi
  [ "$new" = "$base" ] && continue
  mv "$path" "$WORK/$new"
  renamed+=("${base}=${new}")
done

echo "重命名 ${#renamed[@]} 个产物："
printf '  %s\n' "${renamed[@]}"

# latest.json 的 url 指回旧文件名，同步替换后再覆盖上传
if [ -f "$WORK/latest.json" ] && [ "${#renamed[@]}" -gt 0 ]; then
  python3 - "$WORK/latest.json" "${renamed[@]}" <<'PY'
import sys

path, pairs = sys.argv[1], sys.argv[2:]
text = open(path, encoding="utf-8").read()
for pair in pairs:
    old, new = pair.split("=", 1)
    text = text.replace(old, new)
open(path, "w", encoding="utf-8").write(text)
PY
fi

# 先传新名字再删旧名字：中途失败时至少新产物已就位，不会留下空 release
gh release upload "$TAG" "$WORK"/* --clobber

for pair in "${renamed[@]}"; do
  gh release delete-asset "$TAG" "${pair%%=*}" --yes
done

echo "完成：$TAG 的产物已统一命名"
