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
        // -d 指定起始目录（wt.exe 不继承 process::Command 的 current_dir）
        let mut proc = Command::new("wt");
        let mut args: Vec<&str> = Vec::new();
        if !dir.is_empty() {
            args.push("-d");
            args.push(dir);
        }
        args.extend_from_slice(&[
            "powershell",
            "-NoExit",
            "-ExecutionPolicy", "Bypass",
            "-File", ps1_str.as_ref(),
        ]);
        proc.args(&args);

        match proc.spawn() {
            Ok(_) => Ok(format!("已启动：{} @ {}", config.name, directory)),
            Err(e) => Err(format!("启动失败：{}", e)),
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;

        let script_path = std::env::temp_dir()
            .join(format!("aicode_launcher_{}.command", uuid::Uuid::new_v4()));
        let mut lines = vec![
            "#!/bin/zsh".to_string(),
            "set -e".to_string(),
        ];

        for (key, value) in &api_env {
            lines.push(format!(
                "export {}='{}'",
                key,
                value.replace('\'', "'\\''"),
            ));
        }

        if !directory.is_empty() {
            lines.push(format!(
                "cd '{}'",
                directory.replace('\'', "'\\''"),
            ));
        }

        lines.push("set +e".to_string());
        lines.push(command.clone());
        lines.push("exit_code=$?".to_string());
        lines.push("echo".to_string());
        lines.push(r#"echo "Command exited with status ${exit_code}. Interactive shell remains open.""#.to_string());
        lines.push(r#"exec /bin/zsh -i"#.to_string());
        let script = lines.join("\n") + "\n";

        std::fs::write(&script_path, script)
            .map_err(|e| format!("写临时脚本失败：{}", e))?;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置脚本权限失败：{}", e))?;

        match Command::new("open")
            .arg("-a")
            .arg("Terminal")
            .arg(&script_path)
            .spawn()
        {
            Ok(_) => Ok(format!("已启动：{} @ {}", config.name, directory)),
            Err(e) => Err(format!("启动失败：{}", e)),
        }
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
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
