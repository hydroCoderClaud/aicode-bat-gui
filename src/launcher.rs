// CLI 启动模块
use crate::config::Config;
use std::collections::HashMap;
use std::process::Command;

/// 启动 CLI 工具
pub fn launch_cli(
    config: &Config,
    directory: &str,
    api_env: HashMap<String, String>,
    command: String,
) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        // 把环境变量写入临时 bat 文件
        // SET "KEY=VALUE" 语法可以正确处理值中含 &、| 等特殊字符
        let mut lines: Vec<String> = api_env
            .iter()
            .map(|(k, v)| format!("SET \"{}={}\"", k, v))
            .collect();
        lines.push(command.clone());
        let bat_content = lines.join("\r\n") + "\r\n";

        let bat_path = std::env::temp_dir().join("aicode_launcher_run.bat");
        std::fs::write(&bat_path, bat_content)
            .map_err(|e| format!("写临时文件失败: {}", e))?;

        let bat_str = bat_path.to_string_lossy();
        let dir = directory.trim_end_matches(['\\', '/']);

        // 用逐参数形式传给 cmd，避免 Rust 的 \" 转义与 cmd.exe 解析规则冲突。
        // start 会创建完全独立的 CMD 窗口，关闭 launcher 后新窗口仍可正常输入。
        let mut proc = Command::new("cmd");
        proc.args(["/c", "start", "", "cmd", "/k", bat_str.as_ref()]);
        if !dir.is_empty() {
            proc.current_dir(dir);
        }

        match proc.spawn() {
            Ok(_) => Ok(format!("已启动：{} @ {}", config.name, directory)),
            Err(e) => Err(format!("启动失败: {}", e)),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut parts: Vec<String> = api_env
            .iter()
            .map(|(k, v)| format!("export {}='{}'", k, v.replace('\'', "'\\''")))
            .collect();
        if !directory.is_empty() {
            parts.push(format!("cd '{}'", directory.replace('\'', "'\\''")));
        }
        parts.push(command.clone());
        let inline_cmd = parts.join(" && ");

        let mut proc = Command::new("sh");
        proc.arg("-c").arg(&inline_cmd).envs(&api_env);
        if !directory.is_empty() {
            proc.current_dir(directory);
        }

        match proc.spawn() {
            Ok(_) => Ok(format!("已启动：{} @ {}", config.name, directory)),
            Err(e) => Err(format!("启动失败：{}", e)),
        }
    }
}

