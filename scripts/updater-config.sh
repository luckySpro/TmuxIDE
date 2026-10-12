#!/usr/bin/env bash
# 配置了升级公钥和签名私钥时，让 tauri build 额外生成签名的升级包；否则不输出任何参数。
set -euo pipefail
cd "$(dirname "$0")/.."
pub=$(tr -d '[:space:]' < crates/tmuxide-update/updater.pub)
if [ -n "$pub" ] && [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "--config {\"bundle\":{\"createUpdaterArtifacts\":true},\"plugins\":{\"updater\":{\"pubkey\":\"$pub\"}}}"
elif [ -n "$pub" ]; then
  echo "::warning::没有 TAURI_SIGNING_PRIVATE_KEY，跳过桌面版升级包" >&2
fi
