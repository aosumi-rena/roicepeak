# ROICEPEAK

<p align="center">
  <img src="assets/branding/app-icon.png" alt="ROICEPEAK 图标" width="96" height="96">
</p>

使用 Rust + egui 开发的语音合成引擎与编辑器。

## 构建与运行

需要 Rust 1.99.0，版本由 `rust-toolchain.toml` 指定。

### Windows

需要 MSVC、Windows SDK 和支持 DX12 的显卡。

```powershell
cargo run --release
```

### Linux

支持 X11 / Wayland，使用 Vulkan 或 OpenGL。Ubuntu 24.04 构建示例：

```sh
sudo apt install build-essential pkg-config libasound2-dev libwayland-dev libxkbcommon-dev \
  fontconfig fonts-noto-cjk libvulkan1 libegl1 libgl1 xdg-desktop-portal xdg-desktop-portal-gtk
cargo build --release --locked --bins
./target/release/roicepeak
```

需要可用的图形驱动、ALSA 音频设备和桌面对应的 XDG 门户后端（KDE 使用 `xdg-desktop-portal-kde`）。

## 声库与配置

首次使用请在设置中手动选择声库目录和发音词典目录，程序不会自动探测资源路径，也不附带声库或词典。

配置目录：Windows 为 `%LOCALAPPDATA%/ROICEPEAK`，Linux 为 `$XDG_CONFIG_HOME/ROICEPEAK`（未设置时为 `$HOME/.config/ROICEPEAK`）。

命令行合成与诊断工具的用法可通过 `cargo run --release --locked --bin roicepeak-synth -- --help` 查看。
