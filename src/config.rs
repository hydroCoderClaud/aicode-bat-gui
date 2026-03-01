// 配置文件结构定义
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub vendor: String,
    pub command: String,
    pub env_base_url: String,
    pub env_auth_token: String,
    pub env_api_key: String,
    pub env_proxy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub tool: String,
    pub base_url: String,
    pub key: String,
    pub key_type: String,
    pub proxy: String,
    pub extra_env: HashMap<String, String>,
    pub command_args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Global {
    pub last_directory: String,
    pub default_config: String,
    #[serde(default)]
    pub test_model: String,
    #[serde(default)]
    pub test_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfigData {
    pub global: Global,
    pub tools: Vec<Tool>,
    pub configs: Vec<Config>,
}

/// 配置管理器
pub struct ConfigManager {
    pub data: LauncherConfigData,
    pub config_path: String,
}

impl ConfigManager {
    pub fn new(config_path: String) -> Self {
        let mut manager = Self {
            data: LauncherConfigData {
                global: Global {
                    last_directory: String::new(),
                    default_config: String::new(),
                    test_model:     "claude-haiku-4-5".to_string(),
                    test_timeout_secs: 10,
                },
                tools: vec![],
                configs: vec![],
            },
            config_path,
        };
        manager.load();
        manager
    }

    pub fn load(&mut self) {
        if std::path::Path::new(&self.config_path).exists() {
            if let Ok(content) = std::fs::read_to_string(&self.config_path) {
                if let Ok(data) = serde_json::from_str(&content) {
                    self.data = data;
                }
            }
        }
        if self.data.global.test_model.trim().is_empty() {
            self.data.global.test_model = "claude-haiku-4-5".to_string();
        }
        if self.data.global.test_timeout_secs == 0 {
            self.data.global.test_timeout_secs = 10;
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let content = serde_json::to_string_pretty(&self.data)
            .map_err(|e| format!("序列化失败：{}", e))?;
        std::fs::write(&self.config_path, content)
            .map_err(|e| format!("写入失败：{}", e))?;
        Ok(())
    }

    // --- configs 操作 ---
    pub fn add_config(&mut self, mut config: Config) {
        if config.id.is_empty() {
            config.id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        }
        self.data.configs.push(config);
        let _ = self.save();
    }

    pub fn update_config(&mut self, config_id: &str, updated: Config) {
        for (i, c) in self.data.configs.iter_mut().enumerate() {
            if c.id == config_id {
                self.data.configs[i] = updated;
                let _ = self.save();
                return;
            }
        }
    }

    pub fn delete_config(&mut self, config_id: &str) {
        self.data.configs.retain(|c| c.id != config_id);
        let _ = self.save();
    }

    pub fn move_config_up(&mut self, id: &str) {
        if let Some(pos) = self.data.configs.iter().position(|c| c.id == id) {
            if pos > 0 {
                self.data.configs.swap(pos, pos - 1);
                let _ = self.save();
            }
        }
    }

    pub fn move_config_down(&mut self, id: &str) {
        if let Some(pos) = self.data.configs.iter().position(|c| c.id == id) {
            if pos + 1 < self.data.configs.len() {
                self.data.configs.swap(pos, pos + 1);
                let _ = self.save();
            }
        }
    }

    pub fn add_tool(&mut self, tool: Tool) {
        self.data.tools.push(tool);
        let _ = self.save();
    }

    pub fn delete_tool(&mut self, command: &str) {
        self.data.tools.retain(|t| t.command != command);
        let _ = self.save();
    }

    pub fn find_config(&self, config_id: &str) -> Option<&Config> {
        self.data.configs.iter().find(|c| c.id == config_id)
    }

    // --- tools 操作 ---
    pub fn find_tool(&self, command: &str) -> Option<&Tool> {
        self.data.tools.iter().find(|t| t.command == command)
    }

    // --- global 操作 ---
    pub fn find_default_config(&self) -> Option<&Config> {
        let cfg_id = &self.data.global.default_config;
        if !cfg_id.is_empty() {
            if let Some(c) = self.find_config(cfg_id) {
                return Some(c);
            }
        }
        self.data.configs.first()
    }

    // --- 构建环境 ---
    pub fn build_api_env(&self, config: &Config) -> HashMap<String, String> {
        let mut api_env = HashMap::new();

        if let Some(tool) = self.find_tool(&config.tool) {
            if !config.base_url.is_empty() && !tool.env_base_url.is_empty() {
                api_env.insert(tool.env_base_url.clone(), config.base_url.clone());
            }
            if !config.key.is_empty() {
                let env_key = if config.key_type == "auth_token" {
                    tool.env_auth_token.clone()
                } else {
                    tool.env_api_key.clone()
                };
                if !env_key.is_empty() {
                    api_env.insert(env_key, config.key.clone());
                }
            }
            if !config.proxy.is_empty() && !tool.env_proxy.is_empty() {
                api_env.insert(tool.env_proxy.clone(), config.proxy.clone());
            }
        }

        for (k, v) in &config.extra_env {
            api_env.insert(k.clone(), v.clone());
        }

        api_env
    }

    pub fn build_command(&self, config: &Config) -> String {
        let tool_cmd = self.find_tool(&config.tool)
            .map(|t| t.command.clone())
            .unwrap_or_else(|| config.tool.clone());

        if config.command_args.is_empty() {
            tool_cmd
        } else {
            format!("{} {}", tool_cmd, config.command_args)
        }
    }
}
