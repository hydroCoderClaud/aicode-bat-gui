// 密码助手：数据结构与管理器
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeychainEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 有序键值对 [[key, value], ...]
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeychainData {
    pub entries: Vec<KeychainEntry>,
}

pub struct KeychainManager {
    pub data: KeychainData,
    pub config_path: String,
}

impl KeychainManager {
    pub fn new(config_path: String) -> Self {
        let mut mgr = Self {
            data: KeychainData { entries: vec![] },
            config_path,
        };
        mgr.load();
        mgr
    }

    pub fn load(&mut self) {
        if std::path::Path::new(&self.config_path).exists() {
            if let Ok(content) = std::fs::read_to_string(&self.config_path) {
                if let Ok(data) = serde_json::from_str(&content) {
                    self.data = data;
                }
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let content = serde_json::to_string_pretty(&self.data)
            .map_err(|e| format!("序列化失败：{}", e))?;
        std::fs::write(&self.config_path, content)
            .map_err(|e| format!("写入失败：{}", e))?;
        Ok(())
    }

    pub fn add_entry(&mut self, mut entry: KeychainEntry) {
        if entry.id.is_empty() {
            entry.id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        }
        self.data.entries.push(entry);
        let _ = self.save();
    }

    pub fn update_entry(&mut self, id: &str, updated: KeychainEntry) {
        if let Some(e) = self.data.entries.iter_mut().find(|e| e.id == id) {
            *e = updated;
            let _ = self.save();
        }
    }

    pub fn delete_entry(&mut self, id: &str) {
        self.data.entries.retain(|e| e.id != id);
        let _ = self.save();
    }

    pub fn find_entry(&self, id: &str) -> Option<&KeychainEntry> {
        self.data.entries.iter().find(|e| e.id == id)
    }

    /// 大小写不敏感搜索，匹配 name、tags、fields 的 key 和 value
    pub fn search(&self, query: &str, filter_tag: &str) -> Vec<&KeychainEntry> {
        let q = query.to_lowercase();
        self.data.entries.iter().filter(|e| {
            // 标签筛选
            if !filter_tag.is_empty() && !e.tags.iter().any(|t| t == filter_tag) {
                return false;
            }
            // 空搜索词 → 显示全部（已通过标签筛选）
            if q.is_empty() {
                return true;
            }
            // 匹配 name
            if e.name.to_lowercase().contains(&q) {
                return true;
            }
            // 匹配 tags
            if e.tags.iter().any(|t| t.to_lowercase().contains(&q)) {
                return true;
            }
            // 匹配 fields key/value
            if e.fields.iter().any(|(k, v)| {
                k.to_lowercase().contains(&q) || v.to_lowercase().contains(&q)
            }) {
                return true;
            }
            false
        }).collect()
    }

    /// 收集所有已使用的标签（去重）
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.data.entries.iter()
            .flat_map(|e| e.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }
}
