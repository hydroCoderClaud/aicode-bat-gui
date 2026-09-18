// 配置文件结构定义
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, BTreeMap};

/// AICLI 只服务 Claude Code，启动命令与环境变量名固定
pub const CLAUDE_COMMAND: &str = "claude";
const ENV_BASE_URL:   &str = "ANTHROPIC_BASE_URL";
const ENV_AUTH_TOKEN: &str = "ANTHROPIC_AUTH_TOKEN";
const ENV_API_KEY:    &str = "ANTHROPIC_API_KEY";
const ENV_PROXY:      &str = "HTTPS_PROXY";

/// 历史遗留的工具定义。AICLI 已收敛为 Claude 专用，此结构不再参与启动逻辑，
/// 仅用于读写 JSON 时原样保留旧配置数据。
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
    pub extra_env: BTreeMap<String, String>,
    pub command_args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Global {
    pub last_directory: String,
    pub default_config: String,
    #[serde(default)]
    pub backup_directory: String,  // 额外的备份目录，为空则只备份到程序目录下的 backup/
}

fn default_env_hints() -> String {
    "ANTHROPIC_MODEL=Claude - 覆盖默认模型\n\
     ANTHROPIC_DEFAULT_OPUS_MODEL=Claude - 映射 Opus 到指定模型\n\
     ANTHROPIC_DEFAULT_SONNET_MODEL=Claude - 映射 Sonnet 到指定模型\n\
     ANTHROPIC_DEFAULT_HAIKU_MODEL=Claude - 映射 Haiku 到指定模型\n\
     CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=Claude - 上下文压缩触发百分比（0-100）"
        .to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfigData {
    pub global: Global,
    pub tools: Vec<Tool>,
    pub configs: Vec<Config>,
    #[serde(default = "default_env_hints")]
    pub env_hints: String,
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
                    backup_directory: String::new(),
                },
                tools: vec![],
                configs: vec![],
                env_hints: default_env_hints(),
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
        self.prune_legacy_env_hints();
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

    /// 旧版本的常用环境变量提示里含非 Claude CLI 的条目，这里清理掉
    fn prune_legacy_env_hints(&mut self) {
        let hints = &self.data.env_hints;
        if !hints.contains("OPENAI_MODEL") && !hints.contains("GEMINI_MODEL") {
            return;
        }
        let cleaned = hints
            .lines()
            .filter(|l| !l.contains("OPENAI_MODEL") && !l.contains("GEMINI_MODEL"))
            .collect::<Vec<_>>()
            .join("\n");
        self.data.env_hints = cleaned;
        let _ = self.save();
    }

    pub fn find_config(&self, config_id: &str) -> Option<&Config> {
        self.data.configs.iter().find(|c| c.id == config_id)
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
    /// 固定注入 Claude Code 的环境变量
    pub fn build_api_env(&self, config: &Config) -> HashMap<String, String> {
        let mut api_env = HashMap::new();

        if !config.base_url.is_empty() {
            api_env.insert(ENV_BASE_URL.to_string(), config.base_url.clone());
        }
        if !config.key.is_empty() {
            let env_key = if config.key_type == "auth_token" { ENV_AUTH_TOKEN } else { ENV_API_KEY };
            api_env.insert(env_key.to_string(), config.key.clone());
        }
        if !config.proxy.is_empty() {
            api_env.insert(ENV_PROXY.to_string(), config.proxy.clone());
        }

        for (k, v) in &config.extra_env {
            api_env.insert(k.clone(), v.clone());
        }

        api_env
    }

    /// 启动命令固定为 claude，配置里的 tool 字段仅作历史记录
    pub fn build_command(&self, config: &Config) -> String {
        if config.command_args.is_empty() {
            CLAUDE_COMMAND.to_string()
        } else {
            format!("{} {}", CLAUDE_COMMAND, config.command_args)
        }
    }
}
