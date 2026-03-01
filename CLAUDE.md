# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`aicode-bat-gui` is a Windows desktop GUI application for managing and launching AI coding CLI tools (Claude Code, Qwen Coder, Gemini CLI, etc.). Users configure multiple "profiles" with different API endpoints, keys, and proxy settings, then launch the CLI tool in a new CMD window with the correct environment variables injected.

## Tech Stack

- **Frontend**: React 19 + TypeScript + Vite (port 1422) + Tailwind CSS v4
- **Backend**: Rust via Tauri 2
- **IPC**: Tauri `invoke()` for frontend→backend calls

## Development Commands

```bash
# Start dev mode (Vite + Tauri hot reload)
npm run tauri dev

# Build production app
npm run tauri build

# Frontend only (no Tauri window)
npm run dev

# Type-check frontend
npx tsc --noEmit
```

## Architecture

### Frontend (`src/`)
Single-file app in `src/App.tsx`. All state lives in one React component with no routing:
- Left sidebar: config list
- Right panel: form for editing the selected config
- Bottom: status bar

The frontend calls Tauri commands via `invoke()` and mirrors the Rust data types via TypeScript interfaces at the top of `App.tsx`.

### Backend (`src-tauri/src/`)

| File | Responsibility |
|------|----------------|
| `lib.rs` | Tauri app bootstrap, config path resolution, plugin registration |
| `config.rs` | Data structures (`Tool`, `Config`, `Global`, `LauncherConfigData`) + `ConfigManager` CRUD |
| `commands.rs` | `#[tauri::command]` handlers exposed to frontend (`get_config_data`, `save_config`, `delete_config`, `update_global`, `test_connection`, `launch`, `get_config_path`) |
| `launcher.rs` | Windows-specific launch: writes a `.bat` to `%TEMP%`, spawns it via `cmd /c` with `CREATE_NEW_CONSOLE` flag |
| `api_test.rs` | Async HTTP validation for Anthropic, OpenAI-compatible, Gemini, and generic APIs using `reqwest` |

### Config File (`launcher_config.json`)

Persisted automatically. Location priority: exe directory → cwd → `%AppData%\aicode-bat-gui\`.

Structure:
```json
{
  "global": { "last_directory": "...", "default_config": "<id>" },
  "tools": [{ "vendor": "Anthropic", "command": "claude", "env_base_url": "ANTHROPIC_BASE_URL", "env_auth_token": "ANTHROPIC_AUTH_TOKEN", "env_api_key": "ANTHROPIC_API_KEY", "env_proxy": "HTTPS_PROXY" }],
  "configs": [{ "id": "abc12345", "name": "My Config", "tool": "claude", "base_url": "...", "key": "...", "key_type": "auth_token|api_key", "proxy": "...", "extra_env": {}, "command_args": "" }]
}
```

### Key Design Decisions

- **Mutex + lock-release pattern**: `AppState` wraps `ConfigManager` in a `Mutex`. Async commands (`test_connection`, `launch`) explicitly drop the lock before calling async functions to avoid holding it across `.await` points.
- **Launch mechanism**: Rather than exec in-process, a `.bat` file is written to `%TEMP%` with environment variable `SET` commands, then executed in a new CMD window so the user can see output and interact with the CLI.
- **`FrontConfig` vs `Config`**: The frontend-facing structs in `commands.rs` mirror the internal `config.rs` structs 1:1 (currently identical). The separation exists for potential future divergence.
- **Tool definitions are data-driven**: Adding support for a new CLI tool only requires adding an entry to the `tools` array in `launcher_config.json` — no code changes needed.
