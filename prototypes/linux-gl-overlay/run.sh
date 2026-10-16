#!/usr/bin/env bash
# Run the prototype under Xvfb, screenshot the screen while it plays, and check two
# pixels: inside the page's red box (the webview draws), and in the transparent rest
# of the page (the video must show through: green).
set -euo pipefail
out=${1:-out}; mkdir -p "$out"
bin=target/release/linux-gl-overlay
export PROTO_SECONDS=14
Xvfb :99 -screen 0 1024x768x24 >/dev/null 2>&1 & xvfb=$!
export DISPLAY=:99
sleep 2
"$bin" >"$out/log.txt" 2>&1 & app=$!
sleep 9
import -window root "$out/screen.png"
wait $app || true
kill $xvfb || true
cat "$out/log.txt"
red=$(convert "$out/screen.png" -format '%[pixel:p{100,50}]' info:)
video=$(convert "$out/screen.png" -format '%[pixel:p{500,320}]' info:)
echo "page box pixel: $red"
echo "video area pixel: $video"
echo "page=$red video=$video" >"$out/pixels.txt"
