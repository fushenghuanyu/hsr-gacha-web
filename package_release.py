"""
将分发给用户所需的文件打 zip，放在本目录（hsr-gacha-web 根）下。
只包含 hsr-gacha-launcher（不含 hsr-gacha-api）。
解包后目录：exe 与 dist/、resources/ 同层（与 paths.rs 一致）；userData/ 在首次运行后生成。

用法:
  python package_release.py
  python package_release.py --build   # 先执行 npm run build 与 cargo build --release
"""

from __future__ import annotations

import argparse
import datetime as dt
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

LAUNCHER_STEM = "hsr-gacha-launcher"
PACK_NAME = "hsr-gacha"


def launcher_name() -> str:
    return LAUNCHER_STEM + (".exe" if os.name == "nt" else "")


def sh(cmd: list[str], cwd: Path) -> None:
    p = subprocess.run(cmd, cwd=str(cwd))
    if p.returncode != 0:
        sys.exit(p.returncode)


def main() -> None:
    parser = argparse.ArgumentParser(description="打分发 zip（仅含 GUI 启动器 + 资源）。")
    parser.add_argument(
        "-b",
        "--build",
        action="store_true",
        help="先运行 frontend npm run build 与 backend-rust cargo build --release",
    )
    args = parser.parse_args()

    project_root = Path(__file__).resolve().parent
    release_dir = project_root / "backend-rust" / "target" / "release"
    frontend = project_root / "frontend"
    dist_index = project_root / "dist" / "index.html"
    exe_path = release_dir / launcher_name()

    if args.build:
        if shutil.which("npm") is None:
            print("未找到 npm，请先安装 Node.js。", file=sys.stderr)
            sys.exit(1)
        if shutil.which("cargo") is None:
            print("未找到 cargo，请先安装 Rust 工具链。", file=sys.stderr)
            sys.exit(1)
        print(">>> frontend: npm run build")
        sh([shutil.which("npm"), "run", "build"], cwd=frontend)
        print(">>> backend-rust: cargo build --release --bin hsr-gacha-launcher")
        sh(
            [shutil.which("cargo"), "build", "--release", "--bin", "hsr-gacha-launcher"],
            cwd=project_root / "backend-rust",
        )

    if not dist_index.is_file():
        print(
            f"缺少: {dist_index}。请先在 frontend 中执行 npm run build，或加上 --build。",
            file=sys.stderr,
        )
        sys.exit(1)

    if not exe_path.is_file():
        print(
            f"缺少发布程序: {exe_path}。请在 backend-rust 中执行："
            f"cargo build --release --bin hsr-gacha-launcher，或加上 --build。",
            file=sys.stderr,
        )
        sys.exit(1)

    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    zip_basename = f"hsr-gacha-distribution-{stamp}"
    out_zip = project_root / f"{zip_basename}.zip"
    if out_zip.exists():
        out_zip.unlink()

    with tempfile.TemporaryDirectory(prefix="hsr-gacha-pack-") as tmp:
        work = Path(tmp)
        pack_dir = work / PACK_NAME
        pack_dir.mkdir(parents=True)

        shutil.copy2(exe_path, pack_dir / exe_path.name)
        print(f"已加入: {exe_path.name}")

        shutil.copytree(project_root / "dist", pack_dir / "dist")
        print("已加入: dist/")

        resources_src = project_root / "resources"
        if resources_src.is_dir():
            shutil.copytree(resources_src, pack_dir / "resources")
            print("已加入: resources/")
        else:
            (pack_dir / "resources").mkdir()
            (pack_dir / "resources" / "icon").mkdir(parents=True)
            print("提示: 项目内无 resources/，已建立空 resources/ 目录（可选）")

        ar = shutil.make_archive(
            str(project_root / zip_basename), "zip", root_dir=work, base_dir=PACK_NAME
        )
        print(f"已生成: {ar}")


if __name__ == "__main__":
    main()
