#!/usr/bin/env bash
#
# 从 SVG 母版重出整套图标。
#
# 为什么要有这个脚本：以前仓库里最大的图标只有 256px，`.icns`/`.ico` 里的小尺寸
# 都是位图缩出来的，边缘一圈毛刺。现在每个尺寸都由 SVG 直接矢量渲染，
# 缩放这一步彻底不存在了。
#
# 渲染器用 headless Chrome —— 这台机器上没有 rsvg-convert / inkscape / magick，
# 而 Chrome 的 SVG 光栅化质量跟它们同级，且几乎人人都装了。
#
# 用法：./scripts/gen-icons.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ICONS="$ROOT/src-tauri/icons"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
if [[ ! -x "$CHROME" ]]; then
  for candidate in \
    "/Applications/Chromium.app/Contents/MacOS/Chromium" \
    "$(command -v google-chrome || true)" \
    "$(command -v chromium || true)"; do
    [[ -n "$candidate" && -x "$candidate" ]] && CHROME="$candidate" && break
  done
fi
[[ -x "$CHROME" ]] || { echo "找不到 Chrome/Chromium，用 CHROME=... 指一个" >&2; exit 1; }

# 把 SVG 按指定像素尺寸矢量渲染成透明底 PNG
render() {
  local svg="$1" size="$2" out="$3"
  local page="$WORK/page-$size-$(basename "$svg" .svg).html"
  cp "$svg" "$WORK/"
  cat >"$page" <<HTML
<!doctype html><meta charset="utf-8">
<style>html,body{margin:0;padding:0;background:transparent}
img{display:block;width:${size}px;height:${size}px}</style>
<img src="$(basename "$svg")">
HTML
  "$CHROME" --headless --disable-gpu --no-sandbox --hide-scrollbars \
    --force-device-scale-factor=1 --default-background-color=00000000 \
    --window-size="$size,$size" --screenshot="$out" "file://$page" >/dev/null 2>&1
  [[ -s "$out" ]] || { echo "渲染 $size 失败" >&2; exit 1; }
}

echo "▸ 渲染满幅版（Windows / Linux / 窗口与托盘图标）"
for size in 16 24 32 48 64 128 256 512 1024; do
  render "$ICONS/master.svg" "$size" "$WORK/flat-$size.png"
done

echo "▸ 渲染 macOS 版（内容 824/1024，留出 Dock 网格的空白）"
for size in 16 32 64 128 256 512 1024; do
  render "$ICONS/master-macos.svg" "$size" "$WORK/mac-$size.png"
done

echo "▸ 写入 PNG"
cp "$WORK/flat-32.png" "$ICONS/32x32.png"
cp "$WORK/flat-128.png" "$ICONS/128x128.png"
cp "$WORK/flat-256.png" "$ICONS/128x128@2x.png"
cp "$WORK/flat-512.png" "$ICONS/icon.png"

echo "▸ 组装 icon.icns"
SET="$WORK/AgentPulse.iconset"
mkdir -p "$SET"
cp "$WORK/mac-16.png"   "$SET/icon_16x16.png"
cp "$WORK/mac-32.png"   "$SET/icon_16x16@2x.png"
cp "$WORK/mac-32.png"   "$SET/icon_32x32.png"
cp "$WORK/mac-64.png"   "$SET/icon_32x32@2x.png"
cp "$WORK/mac-128.png"  "$SET/icon_128x128.png"
cp "$WORK/mac-256.png"  "$SET/icon_128x128@2x.png"
cp "$WORK/mac-256.png"  "$SET/icon_256x256.png"
cp "$WORK/mac-512.png"  "$SET/icon_256x256@2x.png"
cp "$WORK/mac-512.png"  "$SET/icon_512x512.png"
cp "$WORK/mac-1024.png" "$SET/icon_512x512@2x.png"
iconutil -c icns "$SET" -o "$ICONS/icon.icns"

echo "▸ 组装 icon.ico（16/24/32/48/64/128/256 各一张）"
python3 "$ROOT/scripts/make_ico.py" "$ICONS/icon.ico" \
  "$WORK/flat-16.png" "$WORK/flat-24.png" "$WORK/flat-32.png" \
  "$WORK/flat-48.png" "$WORK/flat-64.png" "$WORK/flat-128.png" "$WORK/flat-256.png"

echo "▸ 网页端 favicon"
mkdir -p "$ROOT/public"
cp "$ICONS/master.svg" "$ROOT/public/icon.svg"
cp "$WORK/flat-32.png" "$ROOT/public/favicon.png"

echo "完成："
ls -l "$ICONS"
