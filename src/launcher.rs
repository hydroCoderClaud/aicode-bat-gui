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
        // 把环境变量写入临时 ps1 文件
        // PowerShell 语法：$env:KEY = 'VALUE'，使用双单引号转义值中的单引号
        let mut lines: Vec<String> = api_env
            .iter()
            .map(|(k, v)| format!(r#"$env:{} = '{}'"#, k, v.replace('\'', "''")))
            .collect();
        lines.push(command.clone());
        let ps1_content = lines.join("\n") + "\n";

        let ps1_path = std::env::temp_dir().join("aicode_launcher_run.ps1");
        std::fs::write(&ps1_path, ps1_content)
            .map_err(|e| format!("写临时文件失败：{}", e))?;

        let ps1_str = ps1_path.to_string_lossy();
        let dir = directory.trim_end_matches(['\\', '/']);

        // 使用 Windows Terminal (wt.exe) 启动 PowerShell 配置文件
        // -M 0.5 设置窗口透明度为 50%（可选，如不需要可移除）
        let mut proc = Command::new("wt");
        proc.args([
            "powershell",
            "-NoExit",
            "-ExecutionPolicy", "Bypass",
            "-File", ps1_str.as_ref(),
        ]);
        if !dir.is_empty() {
            proc.current_dir(dir);
        }

        match proc.spawn() {
            Ok(_) => Ok(format!("已启动：{} @ {}", config.name, directory)),
            Err(e) => Err(format!("启动失败：{}", e)),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut parts: Vec<String> = api_env
            .iter()
            .map(|(k, v)| format!("export {}='{}'", k, v.replace('\'', "'\''")))
            .collect();
        if !directory.is_empty() {
            parts.push(format!("cd '{}'", directory.replace('\'', "'\''")));
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
