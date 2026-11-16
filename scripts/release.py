#!/usr/bin/env python3
"""Build matrix, portable archives, and completeness checks for releases."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
VARIANTS = ("tls", "http-only")


def version():
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def targets():
    return json.loads((ROOT / ".github/release-targets.json").read_text())


def asset_name(target, variant):
    suffix = "zip" if "windows" in target else "tar.gz"
    return f"wlu-login-cli-v{version()}-{target}-{variant}.{suffix}"


def prepare():
    if os.environ.get("GITHUB_REF_TYPE") == "tag":
        tag = os.environ["GITHUB_REF_NAME"]
        if tag != f"v{version()}":
            raise SystemExit(f"tag {tag!r} does not match Cargo.toml v{version()}")
    output = {"version": version()}
    for group, entries in targets().items():
        matrix = []
        for entry in entries:
            for variant in VARIANTS:
                item = dict(entry, variant=variant)
                item["build_target"] = entry["target"] + (
                    "." + entry["glibc"] if "glibc" in entry else ""
                )
                item.setdefault("qemu", "")
                matrix.append(item)
        output[group] = json.dumps({"include": matrix}, separators=(",", ":"))
    if destination := os.environ.get("GITHUB_OUTPUT"):
        with open(destination, "a") as file:
            for key, value in output.items():
                file.write(f"{key}={value}\n")
    print(json.dumps(output, indent=2))


def build(target, variant):
    entry = next(item for items in targets().values() for item in items if item["target"] == target)
    build_target = target + ("." + entry["glibc"] if "glibc" in entry else "")
    command = ["cargo", "zigbuild", "--release", "--locked", "--target", build_target]
    if variant == "http-only":
        command.append("--no-default-features")
    subprocess.run(command, cwd=ROOT, check=True)


def verify_elf(path, target):
    data = path.read_bytes()
    if data[:4] != b"\x7fELF":
        raise ValueError(f"{path} is not ELF")
    endian = "<" if data[5] == 1 else ">"
    machine = struct.unpack_from(endian + "H", data, 18)[0]
    expected = {
        "x86_64": 62, "i686": 3, "aarch64": 183, "armv7": 40,
        "arm": 40, "armv5te": 40, "riscv64gc": 243,
        "loongarch64": 258, "s390x": 22,
    }[target.split("-")[0]]
    if machine != expected:
        raise ValueError(f"wrong ELF architecture: {machine}, expected {expected}")
    is_64 = data[4] == 2
    fmt = endian + ("16sHHIQQQIHHHHHH" if is_64 else "16sHHIIIIIHHHHHH")
    header = struct.unpack_from(fmt, data)
    interpreter = None
    needed = []
    for index in range(header[10]):
        offset = header[5] + index * header[9]
        if is_64:
            kind, _, start, _, _, size, _, _ = struct.unpack_from(endian + "IIQQQQQQ", data, offset)
        else:
            kind, start, _, _, size, _, _, _ = struct.unpack_from(endian + "IIIIIIII", data, offset)
        if kind == 3:
            interpreter = data[start:start + size].rstrip(b"\0").decode()
        if kind == 2:
            step, dynamic_fmt = (16, "qQ") if is_64 else (8, "iI")
            for pos in range(start, start + size, step):
                tag, value = struct.unpack_from(endian + dynamic_fmt, data, pos)
                if tag == 0:
                    break
                if tag == 1:
                    needed.append(value)
    if "musl" in target and (interpreter or needed):
        raise ValueError(f"musl build is not fully static: interpreter={interpreter}, needed={needed}")
    print(f"Verified ELF: {target}, interpreter={interpreter}, shared_libraries={len(needed)}")


def package(target, variant):
    executable = "wlu-login-cli.exe" if "windows" in target else "wlu-login-cli"
    binary = ROOT / "target" / target / "release" / executable
    if "linux" in target:
        verify_elf(binary, target)
    revision = os.environ.get("GITHUB_SHA") or subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    info = {
        "version": version(), "target": target, "variant": variant,
        "tls": variant == "tls", "revision": revision,
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "binary_bytes": binary.stat().st_size,
    }
    entry = next(item for items in targets().values() for item in items if item["target"] == target)
    info["glibc_baseline"] = entry.get("glibc")
    members = {
        executable: (binary.read_bytes(), 0o755),
        "README.md": ((ROOT / "README.md").read_bytes(), 0o644),
        "LICENSE": ((ROOT / "LICENSE").read_bytes(), 0o644),
        "config.example.yaml": ((ROOT / "config.example.yaml").read_bytes(), 0o644),
        "build-info.json": ((json.dumps(info, indent=2) + "\n").encode(), 0o644),
    }
    output = ROOT / "dist" / asset_name(target, variant)
    output.parent.mkdir(exist_ok=True)
    if output.suffix == ".zip":
        with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name, (data, mode) in members.items():
                member = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                member.create_system = 3
                member.external_attr = (0o100000 | mode) << 16
                member.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(member, data)
    else:
        with output.open("wb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                    for name, (data, mode) in members.items():
                        member = tarfile.TarInfo(name)
                        member.size, member.mode = len(data), mode
                        archive.addfile(member, io.BytesIO(data))
    print(f"Packaged {output.name}: {output.stat().st_size} bytes")


def assemble():
    dist = ROOT / "dist"
    expected = {
        asset_name(entry["target"], variant): (entry["target"], variant)
        for entries in targets().values() for entry in entries for variant in VARIANTS
    }
    actual = {p.name for p in dist.iterdir() if p.name.endswith((".tar.gz", ".zip"))}
    if actual != expected.keys():
        raise ValueError(f"release assets differ: missing={expected.keys() - actual}, unexpected={actual - expected.keys()}")
    manifest = []
    for name, (target, variant) in sorted(expected.items()):
        path = dist / name
        if name.endswith(".zip"):
            with zipfile.ZipFile(path) as archive:
                info = json.loads(archive.read("build-info.json"))
                binary = archive.read("wlu-login-cli.exe")
        else:
            with tarfile.open(path) as archive:
                info = json.load(archive.extractfile("build-info.json"))
                binary = archive.extractfile("wlu-login-cli").read()
                if archive.getmember("wlu-login-cli").mode & 0o111 != 0o111:
                    raise ValueError(f"{name}: executable permissions missing")
        if (info["version"], info["target"], info["variant"]) != (version(), target, variant):
            raise ValueError(f"{name}: wrong version, target or variant")
        if info["revision"] != os.environ["GITHUB_SHA"]:
            raise ValueError(f"{name}: built from a different commit")
        if hashlib.sha256(binary).hexdigest() != info["binary_sha256"]:
            raise ValueError(f"{name}: corrupt binary")
        manifest.append(dict(info, file=name, bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    manifest_path = dist / "release-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    files = [dist / name for name in sorted(expected)] + [manifest_path]
    (dist / "SHA256SUMS").write_text("".join(
        hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name + "\n" for path in files
    ))
    count = len(expected)
    notes = f"""# wlu-login-cli v{version()}

SRUN 校园网登录命令行工具，支持登录、注销、状态查询、网卡绑定和守护重连。

本次提供 **{count // len(VARIANTS)} 个平台目标、{count} 个构建包**，每个目标均有两种版本：

- **tls**：支持 HTTP 和 HTTPS，推荐使用。
- **http-only**：移除 TLS 以减小体积，仅支持 HTTP。

**OpenWrt / Alpine 请下载对应架构的 `linux-musl` 静态版本。** `linux-gnu` 版本依赖 glibc。
Windows 下载 `.zip`，macOS / Linux 下载 `.tar.gz`。
文件名中的完整 Rust target 标识 CPU 架构、操作系统及 libc/ABI，请勿只按位数选择。

校验方法：`sha256sum -c SHA256SUMS`（在包含全部下载文件的目录中执行）。
`release-manifest.json` 和包内的 `build-info.json` 记录源代码提交、功能、大小和 SHA-256。

```sh
wlu-login-cli net login -u STUDENT_ID -p PASSWORD -a 1 -f eth1 --save
wlu-login-cli net info -a 1 -f eth1
```

macOS、Windows 和 Linux 的原生构建执行单元测试和本地 HTTP 协议验证；
Linux musl 构建另外验证无动态库依赖，并通过 QEMU 执行启动及本地认证流程验证。
尚未在每种实际硬件及校园网环境中逐一测试。
"""
    (ROOT / "release-notes.md").write_text(notes)
    print(f"Verified all {count} release archives and generated SHA256SUMS")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("prepare")
    builder = sub.add_parser("build")
    builder.add_argument("target")
    builder.add_argument("variant", choices=VARIANTS)
    pack = sub.add_parser("package")
    pack.add_argument("target")
    pack.add_argument("variant", choices=VARIANTS)
    elf = sub.add_parser("verify-elf")
    elf.add_argument("binary", type=Path)
    elf.add_argument("target")
    sub.add_parser("assemble")
    args = parser.parse_args()
    if args.command == "prepare":
        prepare()
    elif args.command == "build":
        build(args.target, args.variant)
    elif args.command == "package":
        package(args.target, args.variant)
    elif args.command == "verify-elf":
        verify_elf(args.binary, args.target)
    else:
        assemble()


if __name__ == "__main__":
    main()
