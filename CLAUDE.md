# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`aicode-bat-gui` is a Windows desktop GUI application for managing and launching AI coding CLI tools (Claude Code, Qwen Coder, Gemini CLI, etc.). Users configure multiple "profiles" with different API endpoints, keys, and proxy settings, then launch the CLI tool in a new CMD window with the correct environment variables injected.

## Tech Stack

- **Language**: Pure Rust
- **GUI**: eframe / egui 0.29
- **HTTP**: reqwest (async, with socks proxy support)
- **Async runtime**: tokio
- **System tray**: tray-icon 0.19 + image (ico decoding)
- **Windows integration**: Win32 FFI (FindWindowW, ShowWindow, CreateMutexW, etc.)

## Development Commands

```bash
# Check compilation
cargo check

# Build debug
cargo build

# Build release (must copy to exe/ afterwards)
cargo build --release
cp target/release/aicode-bat-gui.exe exe/aicode-bat-gui.exe
```

**重要**: `cargo build --release` 后必须复制到 `exe/` 目录。

## Architecture

### Source Files (`src/`)

| File | Responsibility |
|------|----------------|
| `main.rs` | 入口、Win32 FFI、单实例检测、系统托盘、egui UI（单文件应用，所有状态在 `LauncherApp` 中） |
| `config.rs` | 数据结构 (`Tool`, `Config`, `Global`, `LauncherConfigData`) + `ConfigManager` CRUD |
| `launcher.rs` | Windows 启动机制：写 `.bat` 到 `%TEMP%`，通过 `cmd /c` + `CREATE_NEW_CONSOLE` 启动 |
| `api_test.rs` | 异步 HTTP 连接测试：Anthropic、OpenAI 兼容、Gemini、通用 API |

### Config File (`launcher_config.json`)

Persisted automatically. Location priority: exe directory (portable) → `%AppData%\aicode-bat-gui\` (installed).

Structure:
```json
{
  "global": { "last_directory": "...", "default_config": "<id>" },
  "tools": [{ "vendor": "Anthropic", "command": "claude", "env_base_url": "ANTHROPIC_BASE_URL", "env_auth_token": "ANTHROPIC_AUTH_TOKEN", "env_api_key": "ANTHROPIC_API_KEY", "env_proxy": "HTTPS_PROXY" }],
  "configs": [{ "id": "abc12345", "name": "My Config", "tool": "claude", "base_url": "...", "key": "...", "key_type": "auth_token|api_key", "proxy": "...", "extra_env": {}, "command_args": "" }]
}
```

### Key Design Decisions

- **Single instance**: 使用 Win32 命名 Mutex (`Global\aicode-bat-gui-single-instance`) 确保只运行一个实例，重复启动时激活已有窗口。
- **System tray**: 关闭按钮隐藏窗口到托盘（`ShowWindow(SW_HIDE)`），而非退出程序。使用 `tray_icon::set_event_handler` 回调处理托盘事件（双击恢复、右键菜单退出），因为窗口隐藏后 egui 的 `update()` 不再被调用，轮询模式无法工作。
- **Launch mechanism**: `.bat` 文件写入 `%TEMP%` 并在新 CMD 窗口执行，用户可看到输出并与 CLI 交互。
- **Tool definitions are data-driven**: 新增 CLI 工具只需在 `tools` 数组添加条目，无需改代码。
- **Window icon**: 窗口标题栏、任务栏、托盘图标统一使用 `assets/app.ico`。
- **Right-click menu**: 使用 Windows 注册表的经典 shell verb 方式实现，无需 COM DLL。
