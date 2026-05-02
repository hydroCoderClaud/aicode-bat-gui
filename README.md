# aicode-bat-gui

Windows 桌面 GUI 应用，用于管理和启动 AI 编码 CLI 工具（Claude Code、Qwen Coder、Gemini CLI 等）。

## 功能

- **多配置管理**：为不同的 AI CLI 工具配置多个 profile，支持不同的 API 端点、密钥和代理设置
- **一键启动**：在新 CMD 窗口中启动 CLI 工具，自动注入环境变量
- **API 连接测试**：支持 Anthropic、OpenAI 兼容、Gemini 等多种 API 的连接测试
- **系统托盘**：最小化到托盘，支持快速恢复
- **右键菜单**：在 Windows 文件浏览器中右键快速启动
- **数据备份**：一键备份配置和密码数据，支持打开备份目录
- **密码管理**：安全存储 API 密钥和认证令牌

## 技术栈

- **语言**：Pure Rust
- **GUI**：eframe / egui 0.29
- **HTTP**：reqwest（异步，支持 SOCKS 代理）
- **异步运行时**：tokio
- **系统托盘**：tray-icon 0.19
- **Windows 集成**：Win32 FFI

## 开发

### 编译

```bash
# 检查编译
cargo check

# 调试构建
cargo build

# Release 构建（必须复制到 exe/ 目录）
cargo build --release
cp target/release/aicode-bat-gui.exe exe/aicode-bat-gui.exe
```

### macOS App

```bash
chmod +x scripts/build-macos-app.sh
./scripts/build-macos-app.sh
open "dist/AICode BAT GUI.app"
```

生成产物位于 `dist/AICode BAT GUI.app`。

### 项目结构

| 文件 | 职责 |
|------|------|
| `src/main.rs` | 入口、Win32 FFI、单实例检测、系统托盘、egui UI |
| `src/config.rs` | 配置数据结构和 CRUD 操作 |
| `src/launcher.rs` | Windows 启动机制 |
| `src/api_test.rs` | 异步 HTTP 连接测试 |
| `src/keychain.rs` | 密码存储管理 |

### 配置文件

配置自动保存到 `launcher_config.json`，位置优先级：exe 目录（便携版）→ `%AppData%\aicode-bat-gui\`（安装版）。

## 平台支持

- **Windows**：完全支持，包含右键菜单注册、托盘隐藏与恢复
- **macOS**：可运行的兼容版本，支持配置管理、密码助手、在 `Terminal.app` 中启动 CLI，以及通过应用内按钮安装 Finder 右键 Quick Action
- **Linux**：基础兼容，非主要支持平台
