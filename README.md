# wlu-login-cli-rs

[![Build and release](https://github.com/syx0310/wlu-login-cli-rs/actions/workflows/release.yml/badge.svg)](https://github.com/syx0310/wlu-login-cli-rs/actions/workflows/release.yml)

西湖大学 SRUN 校园网认证命令行工具，使用 Rust 实现。支持登录、注销、状态查询、自动重连、User-Agent 设置和网卡绑定。

## 下载

从 [GitHub Releases](https://github.com/syx0310/wlu-login-cli-rs/releases) 下载与系统、CPU 架构和 libc 匹配的包。

- `tls`：支持 HTTP / HTTPS，默认推荐。
- `http-only`：移除 TLS 以减小体积，仅支持 HTTP。
- `linux-musl`：**静态链接，适用于 OpenWrt、Alpine 及其他匹配架构的 Linux 系统**。
- `linux-gnu`：动态链接 glibc，普通目标要求 glibc 2.17+；RISC-V 为 2.27+，LoongArch 为 2.36+。包内 `build-info.json` 记录对应基线。
- Windows 使用 `.zip`，Linux / macOS 使用 `.tar.gz`。包内包含可执行文件、配置样本、本文档、许可证和 `build-info.json`。

例如 x86-64 OpenWrt 使用 `wlu-login-cli-v0.0.1-x86_64-unknown-linux-musl-tls.tar.gz`。
同名的 `linux-gnu` 文件依赖 glibc 加载器，在 OpenWrt 上可能出现“文件存在但 not found”。

构建目标（每个目标均提供 `tls` 和 `http-only`）：

| 系统 / CPU | Rust targets |
| --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu` / `x86_64-unknown-linux-musl` |
| Linux x86 32 位 | `i686-unknown-linux-gnu` / `i686-unknown-linux-musl` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` / `aarch64-unknown-linux-musl` |
| Linux ARMv7 hard-float | `armv7-unknown-linux-gnueabihf` / `armv7-unknown-linux-musleabihf` |
| Linux ARM soft-float | `arm-unknown-linux-gnueabi` / `arm-unknown-linux-musleabi` |
| Linux ARM hard-float | `arm-unknown-linux-gnueabihf` / `arm-unknown-linux-musleabihf` |
| Linux ARMv5TE | `armv5te-unknown-linux-gnueabi` / `armv5te-unknown-linux-musleabi` |
| Linux RISC-V 64 | `riscv64gc-unknown-linux-gnu` / `riscv64gc-unknown-linux-musl` |
| Linux LoongArch64 | `loongarch64-unknown-linux-gnu` / `loongarch64-unknown-linux-musl` |
| Linux s390x | `s390x-unknown-linux-gnu` |
| macOS Intel / Apple Silicon | `x86_64-apple-darwin` / `aarch64-apple-darwin` |
| Windows x86-64 / x86 / ARM64 | `x86_64-pc-windows-msvc` / `i686-pc-windows-msvc` / `aarch64-pc-windows-msvc` |

当前矩阵共 24 个目标、48 个构建包。以实际 Release 文件和 `.github/release-targets.json` 为准。

解压后运行 `./wlu-login-cli --help`；Windows 运行 `wlu-login-cli.exe --help`。
`SHA256SUMS` 提供所有发布文件的 SHA-256；单个包可用 `sha256sum 包名` 核对，下载全部文件后可执行 `sha256sum -c SHA256SUMS`。

## 使用

```sh
wlu-login-cli net login --username STUDENT_ID --password PASSWORD -a 1 --save
wlu-login-cli net info -a 1
wlu-login-cli net logout -a 1
wlu-login-cli net internet -a 1
```

默认认证地址是 `http://10.1.10.1`，配置文件是 `$HOME/.wlu-login-cli.yaml`。
`--config FILE` 指定其他配置文件；`--save` 保存本次生效的参数。

仓库和每个发布包均包含 [config.example.yaml](config.example.yaml)。样本如下：

```yaml
net:
  endpoint: "http://10.1.10.1"
  acid: "1"
  ua_preset: "windows-chrome"
  auth:
    username: "YOUR_STUDENT_ID"
    password: "YOUR_PASSWORD"
    interface: "eth1"
```

将样本复制到 `~/.wlu-login-cli.yaml`，填写账号、密码和真实网卡名，即可直接执行 `wlu-login-cli net login`。
无需绑定网卡时删除 `interface`，也可以用 `wlu-login-cli --config ./config.example.yaml net login` 指定配置路径。

### 网卡绑定

OpenWrt / Linux 先查询通往认证服务器的真实设备名：

```sh
ip route get 10.1.10.1
```

如果结果包含 `dev eth1`：

```sh
wlu-login-cli net info -a 1 -f eth1
wlu-login-cli net login -u STUDENT_ID -p PASSWORD -a 1 -f eth1 --save
wlu-login-cli net login -u STUDENT_ID -p PASSWORD -a 1 -f eth1 --daemon --interval 60
```

`-f` / `--interface` 使用系统设备名，例如 `eth1`、`eth0.2`、`pppoe-wan`；OpenWrt 的逻辑接口名 `wan` 不一定是设备名。
Linux 使用 `SO_BINDTODEVICE`，一般需要 root 或相应 capability。macOS / Windows 绑定该网卡的 IPv4 源地址。
示例显式指定 `-a 1`；省略 `-a` 时会另外尝试通过默认网络的百度重定向推断 ac_id。

### HTTPS 和 User-Agent

`tls` 包支持 HTTPS：

```sh
wlu-login-cli net login -e https://10.1.10.1 -a 1 --ua-preset windows-chrome
wlu-login-cli net login -a 1 --user-agent 'Mozilla/5.0 ...'
```

可用预设：`windows-chrome`、`windows-edge`、`mac-safari`、`ios-safari`、`android-chrome`、`none`。

### 仅有 SSH 时上传

在本机将已解压的、匹配远端架构的 Linux 可执行文件通过标准输入上传：

```sh
ssh -T root@ROUTER 'cat > /root/wlu-login-cli.new && chmod 700 /root/wlu-login-cli.new && mv /root/wlu-login-cli.new /root/wlu-login-cli && /root/wlu-login-cli --help' < ./wlu-login-cli
```

## 本地构建

仓库通过 `rust-toolchain.toml` 固定 Rust 1.92.0，并提交 `Cargo.lock`。

```sh
cargo build --release --locked
cargo build --release --locked --no-default-features  # HTTP 精简版
cargo test --locked
cargo test --locked --no-default-features
```

Linux 交叉编译使用 [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) 和 Zig：

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install cargo-zigbuild==0.24.1 ziglang==0.14.1
rustup target add x86_64-unknown-linux-musl
cargo zigbuild --release --locked --target x86_64-unknown-linux-musl
```

产物为 `target/x86_64-unknown-linux-musl/release/wlu-login-cli`。
构建普通 glibc 包时，使用 `--target x86_64-unknown-linux-gnu.2.17` 等带 glibc 基线的目标。

## GitHub Actions / 自动发布

`Build and release` 在推送 `main`、Pull Request 和手动触发时执行完整构建验证；这些运行只生成 Actions artifacts。
推送 `v*` tag 时，在所有检查成功后自动创建 GitHub Release。

1. 更新 `Cargo.toml` 的版本，并运行 `cargo check` 更新 `Cargo.lock`，提交代码。
2. 推送代码，等待 `main` 的完整构建通过。
3. 创建与 Cargo 版本严格对应的 tag 并推送，例如 `git tag v0.0.1 && git push origin v0.0.1`。

工作流包括格式检查、Clippy、两种功能配置的单元测试、原生执行验证、静态 musl 链接验证、QEMU 本地 SRUN 流程和 Linux 网卡绑定验证。
所有预期包齐全且版本、提交、架构和校验和一致后才会发布；上传完成前保持 draft，避免公开不完整的 Release。
测试使用本地模拟服务和测试凭据，不会连接真实校园网，也不需要账号密码 Secrets。
实际校园网认证及各款硬件仍需在对应环境中验证。

## License

Apache-2.0.
