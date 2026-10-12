#!/usr/bin/env python3
"""生成升级目录：update.json（命令行版与桌面版，随后由 CI 用发布私钥签名）、
latest.json 与 latest-mirror.json（Tauri 升级插件读取，后者的下载地址走 GH-Proxy 加速）。

用法：scripts/update-catalog.py <dist 目录> <版本号，如 v0.3.0> <更新说明 .md> [最低支持版本]

下载地址按正式标签构造（releases/download/vX.Y.Z/文件名），不使用草稿的临时地址。
"""

import datetime
import hashlib
import json
import os
import re
import sys

REPO = "luckySpro/TmuxIDE"
MIRROR = "https://gh-proxy.com/"


def main() -> None:
    dist, tag, notes_path = sys.argv[1], sys.argv[2], sys.argv[3]
    min_version = (sys.argv[4] if len(sys.argv) > 4 else "").strip().lstrip("v") or None
    version = tag.lstrip("v")
    semver = re.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$")
    if not semver.match(version) or (min_version and not semver.match(min_version)):
        sys.exit(f"版本号不正确：{version} / {min_version}")
    base = f"https://github.com/{REPO}/releases/download/v{version}/"
    files = sorted(os.listdir(dist))
    notes = open(notes_path, encoding="utf-8").read().strip()
    pub_date = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")

    def sig(name: str) -> str:
        path = os.path.join(dist, name + ".sig")
        if not os.path.exists(path):
            sys.exit(f"缺少签名文件 {name}.sig")
        return open(path, encoding="utf-8").read().strip()

    def one(pattern: str) -> str:
        found = [f for f in files if re.fullmatch(pattern, f)]
        if len(found) != 1:
            sys.exit(f"应恰好有一个文件匹配 {pattern}，实际：{found}")
        return found[0]

    # 命令行版：压缩包地址、大小与 SHA-256
    cli = {}
    for plat in ["linux-x86_64", "linux-aarch64", "macos-arm64", "macos-x86_64"]:
        name = f"tmuxide-{version}-{plat}.tar.gz"
        path = os.path.join(dist, name)
        if not os.path.exists(path):
            sys.exit(f"缺少命令行版 {name}")
        data = open(path, "rb").read()
        cli[plat] = {"url": base + name, "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}

    # 桌面版：Tauri 平台键 → 安装包（macOS 为 .app.tar.gz，Linux 为 AppImage 与 deb）
    mac_arm = one(r"TmuxIDE_.*_aarch64\.app\.tar\.gz")
    mac_x64 = one(r"TmuxIDE_.*_x64\.app\.tar\.gz")
    appimage = one(r".*_amd64\.AppImage")
    deb = one(r".*_amd64\.deb")
    desktop_files = {
        "darwin-aarch64-app": mac_arm,
        "darwin-aarch64": mac_arm,
        "darwin-x86_64-app": mac_x64,
        "darwin-x86_64": mac_x64,
        "linux-x86_64-appimage": appimage,
        "linux-x86_64": appimage,
        "linux-x86_64-deb": deb,
    }

    catalog = {
        "version": version,
        "min_version": min_version,
        "notes": notes,
        "pub_date": pub_date,
        "cli": cli,
        "desktop": {k: sig(f) for k, f in desktop_files.items()},
    }
    with open(os.path.join(dist, "update.json"), "w", encoding="utf-8") as f:
        json.dump(catalog, f, ensure_ascii=False, indent=2)

    for out, prefix in [("latest.json", ""), ("latest-mirror.json", MIRROR)]:
        latest = {
            "version": version,
            "notes": notes,
            "pub_date": pub_date,
            "platforms": {k: {"signature": sig(f), "url": prefix + base + f} for k, f in desktop_files.items()},
        }
        with open(os.path.join(dist, out), "w", encoding="utf-8") as f:
            json.dump(latest, f, ensure_ascii=False, indent=2)
    print(f"已生成 v{version} 的升级目录（最低支持版本：{min_version or '无'}）")


if __name__ == "__main__":
    main()
