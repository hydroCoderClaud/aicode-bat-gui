#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api_test;
mod config;
mod keychain;
mod launcher;

use eframe::egui;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

// ── Win32 FFI：用于隐藏/显示窗口 ────────────────────────────────────────────
#[cfg(windows)]
extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> isize;
    fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn CreateMutexW(attrs: *const u8, owner: i32, name: *const u16) -> isize;
    fn GetLastError() -> u32;
}
#[cfg(windows)]
const SW_HIDE: i32 = 0;
#[cfg(windows)]
const SW_SHOW: i32 = 5;
#[cfg(windows)]
const ERROR_ALREADY_EXISTS: u32 = 183;

// ── 入口 ─────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result {
    // 单实例检测：用命名 Mutex 确保只有一个实例运行
    #[cfg(windows)]
    {
        let name: Vec<u16> = "Global\\aicode-bat-gui-single-instance\0".encode_utf16().collect();
        unsafe {
            let _handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if GetLastError() == ERROR_ALREADY_EXISTS {
                // 已有实例运行：将右键目录写入临时文件，激活窗口后退出
                if let Some(dir) = std::env::args().nth(1).filter(|p| std::path::Path::new(p).is_dir()) {
                    let path = std::env::temp_dir().join("aicode-bat-gui-open-dir.txt");
                    let _ = std::fs::write(&path, &dir);
                }
                let title: Vec<u16> = "CLI 启动管理器\0".encode_utf16().collect();
                let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                if hwnd != 0 {
                    ShowWindow(hwnd, SW_SHOW);
                    SetForegroundWindow(hwnd);
                }
                std::process::exit(0);
            }
            // _handle 故意不关闭，进程结束时自动释放
        }
    }

    // 加载窗口图标
    let icon_data = image::load_from_memory(include_bytes!("../assets/app.ico"))
        .map(|img| {
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            egui::IconData { rgba: rgba.into_raw(), width: w, height: h }
        })
        .ok();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("CLI 启动管理器")
        .with_inner_size([860.0, 600.0])
        .with_min_inner_size([640.0, 420.0]);
    if let Some(icon) = icon_data {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "CLI 启动管理器",
        options,
        Box::new(|cc| {
            load_cjk_font(&cc.egui_ctx);
            Ok(Box::new(LauncherApp::new()))
        }),
    )
}

/// 从 Windows 系统字体目录加载 CJK 字体，使中文正常显示
fn load_cjk_font(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",   // 微软雅黑
        r"C:\Windows\Fonts\simsun.ttc", // 宋体
        r"C:\Windows\Fonts\simhei.ttf", // 黑体
    ];
    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("cjk".to_owned(), egui::FontData::from_owned(data));
            for list in fonts.families.values_mut() {
                list.insert(0, "cjk".to_owned());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

// ── 应用状态 ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum AppTab {
    ConfigManager,
    Keychain,
}

struct LauncherApp {
    active_tab:     AppTab,
    config_mgr:     config::ConfigManager,
    selected_id:    String,
    form:           Option<EditForm>,
    status:         String,
    dark_mode:      bool,
    pending_delete: bool,
    test_rx:        Option<mpsc::Receiver<Result<String, String>>>,
    show_tool_mgr:  bool,
    new_tool_form:  NewToolForm,
    // 系统托盘
    tray_icon:      Option<tray_icon::TrayIcon>,
    menu_quit_id:   Option<tray_icon::menu::MenuId>,
    hwnd:           isize,
    tray_quit:      Arc<AtomicBool>,
    tray_switch_tab: Arc<AtomicU8>,  // 0=无, 1=AICLI启动, 2=密码助手
    // 密码助手
    keychain_mgr:       keychain::KeychainManager,
    kc_selected_id:     String,
    kc_search:          String,
    kc_edit_form:       Option<KcEditForm>,
    kc_filter_tag:      String,
    kc_visible_fields:  HashSet<usize>,
    kc_pending_delete:  bool,
}

#[derive(Clone, Default)]
struct NewToolForm {
    vendor:         String,
    command:        String,
    env_base_url:   String,
    env_auth_token: String,
    env_api_key:    String,
    env_proxy:      String,
}

#[derive(Clone, Default)]
struct EditForm {
    id:             String,
    name:           String,
    description:    String,
    tool:           String,
    base_url:       String,
    key:            String,
    key_visible:    bool,
    key_type:       String,   // "auth_token" | "api_key"
    proxy:          String,
    command_args:   String,
    extra_env_text: String,   // 每行一个 KEY=VALUE
    directory:      String,
}

#[derive(Clone, Default)]
struct KcEditForm {
    id:        String,
    name:      String,
    tags_text: String,                  // 逗号分隔的标签文本
    fields:    Vec<(String, String)>,   // 有序键值对
}

// ── 初始化 ────────────────────────────────────────────────────────────────────

impl LauncherApp {
    fn new() -> Self {
        let config_mgr = config::ConfigManager::new(resolve_config_path());
        let keychain_mgr = keychain::KeychainManager::new(resolve_keychain_path());

        // 优先级：命令行参数(%V，右键菜单) > cwd(任何合法目录) > 配置记录的上次目录
        // 注：右键 exe 直接打开时 Explorer 会把 CWD 设为 exe 所在目录，这是合法的工作目录；
        //     只排除 Windows 系统目录（从开始菜单启动时 CWD 可能为 System32 等）
        let arg_dir = std::env::args().nth(1)
            .filter(|p| std::path::Path::new(p).is_dir());
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let cwd_is_sys = {
            let lower = cwd.to_lowercase().replace('/', "\\");
            lower.contains("\\windows\\system32")
                || lower.contains("\\windows\\syswow64")
        };
        let last_dir = if let Some(d) = arg_dir {
            d
        } else if !cwd.is_empty() && !cwd_is_sys {
            cwd
        } else {
            config_mgr.data.global.last_directory.clone()
        };

        let (selected_id, form) = config_mgr
            .find_default_config()
            .map(|c| (c.id.clone(), Some(cfg_to_form(c, &last_dir))))
            .unwrap_or_default();

        Self {
            active_tab:     AppTab::ConfigManager,
            config_mgr,
            selected_id,
            form,
            status:         "就绪".into(),
            dark_mode:      false,
            pending_delete: false,
            test_rx:        None,
            show_tool_mgr:  false,
            new_tool_form:  NewToolForm::default(),
            tray_icon:      None,
            menu_quit_id:   None,
            hwnd:           0,
            tray_quit:      Arc::new(AtomicBool::new(false)),
            tray_switch_tab: Arc::new(AtomicU8::new(0)),
            keychain_mgr,
            kc_selected_id:     String::new(),
            kc_search:          String::new(),
            kc_edit_form:       None,
            kc_filter_tag:      String::new(),
            kc_visible_fields:  HashSet::new(),
            kc_pending_delete:  false,
        }
    }
}

// ── 业务逻辑 ──────────────────────────────────────────────────────────────────

impl LauncherApp {
    fn save(&mut self) {
        let form = match &self.form { Some(f) => f.clone(), None => return };
        if form.name.trim().is_empty() {
            self.status = "错误：名称不能为空".into();
            return;
        }
        let cfg  = form_to_cfg(&form);
        let name = cfg.name.clone();
        if self.config_mgr.find_config(&cfg.id).is_some() {
            self.config_mgr.update_config(&cfg.id.clone(), cfg.clone());
            self.status = format!("已更新：{}", name);
        } else {
            self.config_mgr.add_config(cfg.clone());
            self.selected_id = cfg.id.clone();
            self.status = format!("已新增：{}", name);
        }
        // 用保存后的数据刷新表单（含新生成的 id）
        if let Some(saved) = self.config_mgr.find_config(&self.selected_id).cloned() {
            let dir = form.directory.clone();
            self.config_mgr.data.global.last_directory = dir.clone();
            let _ = self.config_mgr.save();  // 更新 last_directory（config 已由 add/update 保存）
            self.form = Some(cfg_to_form(&saved, &dir));
        }
    }

    fn delete(&mut self) {
        let id   = self.selected_id.clone();
        let name = self.config_mgr.find_config(&id)
            .map(|c| c.name.clone()).unwrap_or_default();
        self.config_mgr.delete_config(&id);
        self.selected_id.clear();
        self.form           = None;
        self.pending_delete = false;
        self.status         = format!("已删除：{}", name);
    }

    fn test_connection(&mut self) {
        let form = match &self.form { Some(f) => f.clone(), None => return };
        if form.base_url.is_empty() {
            self.status = "API 地址为空，无法测试".into();
            return;
        }
        self.status = "正在测试连接...".into();

        let (tx, rx) = mpsc::channel();
        self.test_rx = Some(rx);

        let (url, key, key_type, tool) = (
            form.base_url.clone(),
            form.key.clone(),
            form.key_type.clone(),
            form.tool.clone(),
        );
        let proxy = (!form.proxy.is_empty()).then_some(form.proxy.clone());
        // 模型优先级：extra_env ANTHROPIC_MODEL > global.test_model
        let model = form.extra_env_text.lines().find_map(|line| {
            let line = line.trim();
            line.strip_prefix("ANTHROPIC_MODEL=").map(|v| v.trim().to_string())
        }).unwrap_or_else(|| self.config_mgr.data.global.test_model.clone());
        let timeout_secs = self.config_mgr.data.global.test_timeout_secs;

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let res = rt.block_on(async {
                match tool.as_str() {
                    "claude" => api_test::test_anthropic(&url, &key, &key_type, proxy.as_deref(), Some(&model), timeout_secs).await,
                    "qwen" | "codex" => api_test::test_openai(&url, &key, proxy.as_deref(), timeout_secs).await,
                    "gemini" => api_test::test_gemini(&url, &key, proxy.as_deref(), timeout_secs).await,
                    _ => api_test::test_generic(&url, proxy.as_deref(), timeout_secs).await,
                }
            });
            let _ = tx.send(res);
        });
    }

    fn launch(&mut self) {
        let form = match &self.form { Some(f) => f.clone(), None => return };
        let cfg     = form_to_cfg(&form);
        let api_env = self.config_mgr.build_api_env(&cfg);
        let cmd     = self.config_mgr.build_command(&cfg);

        self.config_mgr.data.global.last_directory  = form.directory.clone();
        self.config_mgr.data.global.default_config  = cfg.id.clone();
        let _ = self.config_mgr.save();

        self.status = match launcher::launch_cli(&cfg, &form.directory, api_env, cmd) {
            Ok(msg) => msg,
            Err(e)  => format!("启动失败：{}", e),
        };
    }

    // ── 密码助手 UI ──────────────────────────────────────────────────────────

    fn ui_keychain(&mut self, ctx: &egui::Context) {
        // 首次进入或无选中时，自动选中第一条记录
        if self.kc_edit_form.is_none() && !self.keychain_mgr.data.entries.is_empty() {
            let first = &self.keychain_mgr.data.entries[0];
            self.kc_selected_id = first.id.clone();
            self.kc_edit_form = Some(KcEditForm {
                id: first.id.clone(),
                name: first.name.clone(),
                tags_text: first.tags.join(", "),
                fields: first.fields.clone(),
            });
        }

        let mut do_new = false;
        let mut do_save = false;
        let mut do_delete_click = false;
        let mut do_delete_confirm = false;
        let mut do_delete_cancel = false;
        let mut select_id: Option<String> = None;
        let mut do_add_field = false;
        let mut do_remove_field: Option<usize> = None;
        let mut do_move_field_up: Option<usize> = None;
        let mut do_move_field_down: Option<usize> = None;

        // ── 左侧搜索 + 列表 ─────────────────────────────────────────────
        egui::SidePanel::left("kc_sidebar").width_range(160.0..=280.0).show(ctx, |ui| {
            ui.add_space(4.0);

            // 新增按钮置顶
            if ui.button("➕ 新增记录").clicked() {
                do_new = true;
            }

            ui.add_space(4.0);

            // 搜索框
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(egui::TextEdit::singleline(&mut self.kc_search)
                    .hint_text("搜索...")
                    .desired_width(ui.available_width()));
            });

            ui.add_space(4.0);

            // 标签筛选按钮
            let all_tags = self.keychain_mgr.all_tags();
            if !all_tags.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_label(self.kc_filter_tag.is_empty(), "全部").clicked() {
                        self.kc_filter_tag.clear();
                    }
                    for tag in &all_tags {
                        if ui.selectable_label(self.kc_filter_tag == *tag, tag).clicked() {
                            if self.kc_filter_tag == *tag {
                                self.kc_filter_tag.clear();
                            } else {
                                self.kc_filter_tag = tag.clone();
                            }
                        }
                    }
                });
                ui.add_space(2.0);
            }

            ui.separator();

            // 过滤后的列表（可滚动）
            let filtered: Vec<_> = self.keychain_mgr.search(&self.kc_search, &self.kc_filter_tag)
                .into_iter().map(|e| (e.id.clone(), e.name.clone())).collect();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for (id, name) in &filtered {
                    let selected = self.kc_selected_id == *id;
                    if ui.selectable_label(selected, name).clicked() {
                        select_id = Some(id.clone());
                    }
                }
            });
        });

        // ── 右侧详情（始终可编辑）──────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(form) = &mut self.kc_edit_form {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // 名称 + 标签
                    egui::Grid::new("kc_edit_form")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            ui.label("名称:");
                            ui.add(egui::TextEdit::singleline(&mut form.name)
                                .desired_width(ui.available_width()));
                            ui.end_row();

                            ui.label("标签:");
                            ui.add(egui::TextEdit::singleline(&mut form.tags_text)
                                .hint_text("逗号分隔，如: 云服务, 工作")
                                .desired_width(ui.available_width()));
                            ui.end_row();
                        });

                    ui.separator();
                    ui.add_space(2.0);

                    // 字段：key 和 value 均为可编辑输入框
                    let field_count = form.fields.len();
                    for i in 0..field_count {
                        ui.horizontal(|ui| {
                            // 字段名（固定宽度）
                            ui.add(egui::TextEdit::singleline(&mut form.fields[i].0)
                                .hint_text("字段名")
                                .desired_width(160.0));

                            let secret = is_secret_field(&form.fields[i].0);
                            let visible = self.kc_visible_fields.contains(&i);

                            // 按钮靠右，值输入框填满剩余空间
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("删除").clicked() {
                                    do_remove_field = Some(i);
                                }
                                if i + 1 < field_count {
                                    if ui.small_button("▼").clicked() {
                                        do_move_field_down = Some(i);
                                    }
                                } else {
                                    ui.add_enabled(false, egui::Button::new("▼").small());
                                }
                                if i > 0 {
                                    if ui.small_button("▲").clicked() {
                                        do_move_field_up = Some(i);
                                    }
                                } else {
                                    ui.add_enabled(false, egui::Button::new("▲").small());
                                }
                                if ui.small_button("复制").clicked() {
                                    ui.output_mut(|o| o.copied_text = form.fields[i].1.clone());
                                    self.status = format!("已复制「{}」", form.fields[i].0);
                                }
                                if secret {
                                    let eye = if visible { "🙈" } else { "👁" };
                                    if ui.small_button(eye).clicked() {
                                        if visible {
                                            self.kc_visible_fields.remove(&i);
                                        } else {
                                            self.kc_visible_fields.insert(i);
                                        }
                                    }
                                }
                                if secret && !visible {
                                    ui.add(egui::TextEdit::singleline(&mut form.fields[i].1)
                                        .hint_text("值")
                                        .password(true)
                                        .desired_width(ui.available_width()));
                                } else {
                                    ui.add(egui::TextEdit::multiline(&mut form.fields[i].1)
                                        .hint_text("值")
                                        .desired_rows(1)
                                        .desired_width(ui.available_width()));
                                }
                            });
                        });
                    }

                    ui.add_space(4.0);
                    if ui.button("➕ 新增字段").clicked() {
                        do_add_field = true;
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("💾 保存").clicked() { do_save = true; }

                        if !form.id.is_empty() {
                            if self.kc_pending_delete {
                                if ui.button("⚠ 确认删除").clicked() { do_delete_confirm = true; }
                                if ui.button("取消").clicked() { do_delete_cancel = true; }
                            } else {
                                if ui.button("🗑 删除").clicked() { do_delete_click = true; }
                            }
                        }
                    });
                });
            } else {
                // 无选中
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.label("从左侧选择记录，或点击 [➕ 新增记录]");
                });
            }
        });

        // ── 处理动作 ────────────────────────────────────────────────────
        if let Some(id) = select_id {
            // 选中已有记录 → 自动填充表单
            if let Some(entry) = self.keychain_mgr.find_entry(&id).cloned() {
                self.kc_selected_id = id;
                self.kc_pending_delete = false;
                self.kc_visible_fields.clear();
                self.kc_edit_form = Some(KcEditForm {
                    id: entry.id,
                    name: entry.name,
                    tags_text: entry.tags.join(", "),
                    fields: entry.fields,
                });
            }
        }

        if do_new {
            self.kc_selected_id.clear();
            self.kc_pending_delete = false;
            self.kc_visible_fields.clear();
            self.kc_edit_form = Some(KcEditForm {
                fields: vec![
                    ("网址".into(), String::new()),
                    ("用户名".into(), String::new()),
                    ("密码".into(), String::new()),
                ],
                ..Default::default()
            });
            self.status = "新建密码记录".into();
        }

        if do_add_field {
            if let Some(form) = &mut self.kc_edit_form {
                form.fields.push((String::new(), String::new()));
            }
        }

        if let Some(idx) = do_remove_field {
            if let Some(form) = &mut self.kc_edit_form {
                if idx < form.fields.len() {
                    form.fields.remove(idx);
                }
            }
        }

        if let Some(idx) = do_move_field_up {
            if let Some(form) = &mut self.kc_edit_form {
                if idx > 0 && idx < form.fields.len() {
                    form.fields.swap(idx, idx - 1);
                }
            }
        }

        if let Some(idx) = do_move_field_down {
            if let Some(form) = &mut self.kc_edit_form {
                if idx + 1 < form.fields.len() {
                    form.fields.swap(idx, idx + 1);
                }
            }
        }

        if do_save {
            if let Some(form) = self.kc_edit_form.take() {
                if form.name.trim().is_empty() {
                    self.status = "错误：名称不能为空".into();
                    self.kc_edit_form = Some(form);
                } else {
                    let tags: Vec<String> = form.tags_text
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let entry = keychain::KeychainEntry {
                        id: form.id.clone(),
                        name: form.name.trim().to_string(),
                        tags,
                        fields: form.fields.into_iter()
                            .filter(|(k, _)| !k.trim().is_empty())
                            .collect(),
                    };
                    let name = entry.name.clone();
                    if entry.id.is_empty() {
                        // 新增
                        self.keychain_mgr.add_entry(entry);
                        if let Some(last) = self.keychain_mgr.data.entries.last() {
                            self.kc_selected_id = last.id.clone();
                            // 保存后重新填充表单以获得 id
                            self.kc_edit_form = Some(KcEditForm {
                                id: last.id.clone(),
                                name: last.name.clone(),
                                tags_text: last.tags.join(", "),
                                fields: last.fields.clone(),
                            });
                        }
                        self.status = format!("已新增：{}", name);
                    } else {
                        let id = entry.id.clone();
                        self.keychain_mgr.update_entry(&id, entry);
                        // 保存后刷新表单
                        if let Some(saved) = self.keychain_mgr.find_entry(&id).cloned() {
                            self.kc_edit_form = Some(KcEditForm {
                                id: saved.id,
                                name: saved.name,
                                tags_text: saved.tags.join(", "),
                                fields: saved.fields,
                            });
                        }
                        self.status = format!("已更新：{}", name);
                    }
                    self.kc_visible_fields.clear();
                }
            }
        }

        if do_delete_click { self.kc_pending_delete = true; }
        if do_delete_cancel { self.kc_pending_delete = false; }
        if do_delete_confirm {
            let name = self.keychain_mgr.find_entry(&self.kc_selected_id)
                .map(|e| e.name.clone()).unwrap_or_default();
            self.keychain_mgr.delete_entry(&self.kc_selected_id);
            self.kc_selected_id.clear();
            self.kc_pending_delete = false;
            self.kc_edit_form = None;
            self.kc_visible_fields.clear();
            self.status = format!("已删除：{}", name);
        }
    }
}

// ── UI ────────────────────────────────────────────────────────────────────────

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── 检查其他实例传来的右键打开目录 ───────────────────────────────
        {
            let path = std::env::temp_dir().join("aicode-bat-gui-open-dir.txt");
            if let Ok(dir) = std::fs::read_to_string(&path) {
                let _ = std::fs::remove_file(&path);
                let dir = dir.trim().to_string();
                if !dir.is_empty() {
                    if let Some(form) = &mut self.form {
                        form.directory = dir.clone();
                    }
                    self.config_mgr.data.global.last_directory = dir;
                }
            }
        }

        // ── 首帧初始化托盘图标 ─────────────────────────────────────────────
        #[cfg(windows)]
        if self.hwnd == 0 {
            // 获取窗口句柄
            let title: Vec<u16> = "CLI 启动管理器\0".encode_utf16().collect();
            let h = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
            if h != 0 {
                self.hwnd = h;

                // 解码 ico → RGBA
                if let Ok(img) = image::load_from_memory(include_bytes!("../assets/app.ico")) {
                    let rgba = img.to_rgba8();
                    let (w, h_img) = (rgba.width(), rgba.height());
                    if let Ok(icon) = tray_icon::Icon::from_rgba(rgba.into_raw(), w, h_img) {
                        use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

                        let menu = Menu::new();
                        let item_launch = MenuItem::new("AICLI启动", true, None);
                        let item_keychain = MenuItem::new("密码助手", true, None);
                        let item_quit = MenuItem::new("退出", true, None);
                        let _ = menu.append(&item_launch);
                        let _ = menu.append(&item_keychain);
                        let _ = menu.append(&PredefinedMenuItem::separator());
                        let _ = menu.append(&item_quit);

                        let launch_id = item_launch.id().clone();
                        let keychain_id = item_keychain.id().clone();
                        let quit_id = item_quit.id().clone();
                        self.menu_quit_id = Some(quit_id.clone());

                        // 用 set_event_handler 注册回调：窗口隐藏后 update() 不再被调用，
                        // 但 Windows 消息分发仍会触发这些回调，因此可直接调用 Win32 API。
                        let hwnd_val = h;
                        tray_icon::TrayIconEvent::set_event_handler(Some(move |event: tray_icon::TrayIconEvent| {
                            if let tray_icon::TrayIconEvent::DoubleClick { .. } = event {
                                unsafe {
                                    ShowWindow(hwnd_val, SW_SHOW);
                                    SetForegroundWindow(hwnd_val);
                                }
                            }
                        }));

                        let hwnd_val2 = h;
                        let quit_flag = self.tray_quit.clone();
                        let tab_flag = self.tray_switch_tab.clone();
                        tray_icon::menu::MenuEvent::set_event_handler(Some(move |event: tray_icon::menu::MenuEvent| {
                            if event.id == launch_id {
                                tab_flag.store(1, Ordering::Relaxed);
                                unsafe {
                                    ShowWindow(hwnd_val2, SW_SHOW);
                                    SetForegroundWindow(hwnd_val2);
                                }
                            } else if event.id == keychain_id {
                                tab_flag.store(2, Ordering::Relaxed);
                                unsafe {
                                    ShowWindow(hwnd_val2, SW_SHOW);
                                    SetForegroundWindow(hwnd_val2);
                                }
                            } else if event.id == quit_id {
                                // 设标志 + 显示窗口唤醒事件循环，由 update() 正常 drop 托盘图标
                                quit_flag.store(true, Ordering::Relaxed);
                                unsafe { ShowWindow(hwnd_val2, SW_SHOW); }
                            }
                        }));

                        if let Ok(tray) = tray_icon::TrayIconBuilder::new()
                            .with_tooltip("CLI 启动管理器")
                            .with_icon(icon)
                            .with_menu(Box::new(menu))
                            .build()
                        {
                            self.tray_icon = Some(tray);
                        }
                    }
                }
            }
        }

        // ── 托盘菜单切换标签页 ────────────────────────────────────────────
        #[cfg(windows)]
        match self.tray_switch_tab.swap(0, Ordering::Relaxed) {
            1 => self.active_tab = AppTab::ConfigManager,
            2 => self.active_tab = AppTab::Keychain,
            _ => {}
        }

        // ── 托盘"退出"：drop 托盘图标后正常关闭窗口 ──────────────────
        #[cfg(windows)]
        if self.tray_quit.load(Ordering::Relaxed) {
            self.tray_icon.take(); // Drop → 移除托盘图标
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // ── 拦截关闭按钮 → 隐藏到托盘 ─────────────────────────────────────
        #[cfg(windows)]
        if ctx.input(|i| i.viewport().close_requested()) && self.tray_icon.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            unsafe { ShowWindow(self.hwnd, SW_HIDE); }
        }

        // 轮询异步测试结果
        if let Some(rx) = &self.test_rx {
            match rx.try_recv() {
                Ok(Ok(msg))  => { self.status = format!("✅ {}", msg); self.test_rx = None; }
                Ok(Err(e))   => { self.status = format!("❌ {}", e);   self.test_rx = None; }
                Err(mpsc::TryRecvError::Empty) => ctx.request_repaint(),
                Err(_) => self.test_rx = None,
            }
        }

        // 主题
        ctx.set_visuals(if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        // ── 顶栏 ──────────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("CLI 启动管理器");
                ui.separator();
                ui.selectable_value(&mut self.active_tab, AppTab::ConfigManager, "AICLI启动");
                ui.selectable_value(&mut self.active_tab, AppTab::Keychain, "密码助手");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(if self.dark_mode { "☀" } else { "🌙" }).clicked() {
                        self.dark_mode = !self.dark_mode;
                    }
                    ui.separator();
                    #[cfg(windows)]
                    if ui.button("📌 注册右键菜单").on_hover_text("在文件夹右键菜单中添加「AICode 启动器」入口").clicked() {
                        match register_context_menu() {
                            Ok(_)  => self.status = "✅ 右键菜单注册成功".into(),
                            Err(e) => self.status = format!("❌ 注册失败：{}", e),
                        }
                    }
                    #[cfg(windows)]
                    if ui.button("🗑 卸载右键菜单").clicked() {
                        match unregister_context_menu() {
                            Ok(_)  => self.status = "✅ 右键菜单已卸载".into(),
                            Err(e) => self.status = format!("❌ 卸载失败：{}", e),
                        }
                    }
                    if ui.button("💾 备份").on_hover_text("备份配置和密码数据到 backup 子目录").clicked() {
                        match backup_data_files(
                            &self.config_mgr.config_path,
                            &self.keychain_mgr.config_path,
                        ) {
                            Ok(msg) => self.status = format!("✅ {}", msg),
                            Err(e)  => self.status = format!("❌ 备份失败：{}", e),
                        }
                    }
                });
            });
        });

        // ── 状态栏 ────────────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.test_rx.is_some() { ui.spinner(); }
                ui.label(&self.status);
            });
        });

        // ── 按标签页渲染主内容 ─────────────────────────────────────────────
        match self.active_tab {
        AppTab::ConfigManager => {

        // 收集本帧 UI 动作（避免在持有可变借用时调用 &mut self 方法）
        let mut do_save           = false;
        let mut do_delete_click   = false;
        let mut do_delete_confirm = false;
        let mut do_delete_cancel  = false;
        let mut do_test           = false;
        let mut do_launch         = false;
        let mut select_id: Option<String> = None;
        let mut launch_id: Option<String> = None;
        let mut do_new            = false;
        let mut do_move_up        = false;
        let mut do_move_down      = false;
        let mut do_toggle_tool_mgr = false;
        let mut do_delete_tool: Option<String> = None;
        let mut do_add_tool       = false;

        // ── 左侧配置列表 ──────────────────────────────────────────────────────
        egui::SidePanel::left("sidebar").width_range(160.0..=280.0).show(ctx, |ui| {
            ui.add_space(4.0);
            if ui.button("➕ 新增配置").clicked() {
                do_new = true;
            }
            ui.separator();

            let configs = self.config_mgr.data.configs.clone();
            let sel     = &self.selected_id;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for cfg in &configs {
                    let resp = ui.selectable_label(*sel == cfg.id, &cfg.name);
                    if resp.clicked()        { select_id = Some(cfg.id.clone()); }
                    if resp.double_clicked() { launch_id = Some(cfg.id.clone()); }
                }
            });
        });

        // ── 右侧表单 ──────────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            // 提前取出只读数据，避免与 self.form 的可变借用冲突
            let tools:   Vec<config::Tool> = self.config_mgr.data.tools.clone();
            let testing: bool              = self.test_rx.is_some();

            match &mut self.form {
                None => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 3.0);
                        ui.label("从左侧选择配置，或点击 [➕ 新增配置]");
                    });
                }
                Some(form) => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::Grid::new("form")
                            .num_columns(2)
                            .spacing([8.0, 6.0])
                            .min_col_width(80.0)
                            .show(ui, |ui| {
                                form_row(ui, "名称:", |ui| {
                                    ui.add(egui::TextEdit::singleline(&mut form.name)
                                        .desired_width(f32::INFINITY));
                                });

                                form_row(ui, "描述:", |ui| {
                                    ui.add(egui::TextEdit::singleline(&mut form.description)
                                        .hint_text("备注说明（可选）")
                                        .desired_width(f32::INFINITY));
                                });

                                form_row(ui, "工具:", |ui| {
                                    egui::ComboBox::new("tool_cb", "")
                                        .selected_text(&form.tool)
                                        .show_ui(ui, |ui| {
                                            for t in &tools {
                                                ui.selectable_value(
                                                    &mut form.tool,
                                                    t.command.clone(),
                                                    format!("{} ({})", t.vendor, t.command),
                                                );
                                            }
                                        });
                                    if ui.small_button("管理").clicked() {
                                        do_toggle_tool_mgr = true;
                                    }
                                });

                                form_row(ui, "API 地址:", |ui| {
                                    ui.add(egui::TextEdit::singleline(&mut form.base_url)
                                        .hint_text("https://...")
                                        .desired_width(f32::INFINITY));
                                });

                                form_row(ui, "密钥类型:", |ui| {
                                    ui.radio_value(&mut form.key_type, "auth_token".into(), "auth_token");
                                    ui.radio_value(&mut form.key_type, "api_key".into(),    "api_key");
                                });

                                form_row(ui, "密钥:", |ui| {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let eye = if form.key_visible { "🙈" } else { "👁" };
                                        if ui.small_button(eye).clicked() {
                                            form.key_visible = !form.key_visible;
                                        }
                                        ui.add(
                                            egui::TextEdit::singleline(&mut form.key)
                                                .password(!form.key_visible)
                                                .hint_text("API 密钥")
                                                .desired_width(ui.available_width()),
                                        );
                                    });
                                });

                                form_row(ui, "代理:", |ui| {
                                    ui.add(egui::TextEdit::singleline(&mut form.proxy)
                                        .hint_text("http://127.0.0.1:7890")
                                        .desired_width(f32::INFINITY));
                                });

                                form_row(ui, "命令参数:", |ui| {
                                    ui.add(egui::TextEdit::singleline(&mut form.command_args)
                                        .hint_text("附加到启动命令后，如 --model claude-haiku-4-5 或 --model gemini-3.1-pro-preview")
                                        .desired_width(f32::INFINITY));
                                });

                                form_row(ui, "额外环境变量:", |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut form.extra_env_text)
                                            .hint_text("KEY=VALUE（每行一个）")
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(6),
                                    );
                                });
                            });

                        // 常用环境变量参考
                        ui.add_space(4.0);
                        egui::CollapsingHeader::new("📋 常用环境变量参考")
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("格式：KEY=描述（每行一条）").small().weak());
                                let resp = ui.add(
                                    egui::TextEdit::multiline(&mut self.config_mgr.data.env_hints)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(8)
                                        .font(egui::TextStyle::Monospace),
                                );
                                if resp.changed() {
                                    let _ = self.config_mgr.save();
                                }
                            });
                        ui.add_space(4.0);

                        // 工作目录单独放在 Grid 外，避免与多行文本框重叠
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("工作目录（全局）:");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("选择").clicked() {
                                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                        form.directory = p.to_string_lossy().into_owned();
                                        self.config_mgr.data.global.last_directory = form.directory.clone();
                                        let _ = self.config_mgr.save();
                                    }
                                }
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut form.directory)
                                        .hint_text("启动时的工作目录")
                                        .desired_width(ui.available_width()),
                                );
                                if resp.lost_focus() {
                                    self.config_mgr.data.global.last_directory = form.directory.clone();
                                    let _ = self.config_mgr.save();
                                }
                            });
                        });

                        ui.separator();

                        // 启动命令预览
                        let preview_cmd = self.config_mgr.build_command(&form_to_cfg(form));
                        ui.horizontal(|ui| {
                            ui.label("启动命令:");
                            ui.add(egui::Label::new(
                                egui::RichText::new(&preview_cmd).monospace().weak()
                            ).truncate());
                        });

                        // 按钮栏
                        ui.horizontal(|ui| {
                            if ui.button("💾 保存").clicked() { do_save = true; }

                            // 删除：二次确认
                            if self.pending_delete {
                                if ui.button("⚠ 确认删除").clicked() { do_delete_confirm = true; }
                                if ui.button("取消").clicked()       { do_delete_cancel  = true; }
                            } else {
                                if ui.button("🗑 删除").clicked() { do_delete_click = true; }
                            }

                            ui.add_enabled_ui(!testing, |ui| {
                                if ui.button("🔍 测试连接").clicked() { do_test = true; }
                            });
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.config_mgr.data.global.test_model)
                                    .hint_text("claude-haiku-4-5")
                                    .desired_width(140.0),
                            ).on_hover_text("仅用于 Claude 工具的测试连接，指定发送的模型名（全局设置，不可为空）");
                            if resp.changed() {
                                if self.config_mgr.data.global.test_model.trim().is_empty() {
                                    self.config_mgr.data.global.test_model = "claude-haiku-4-5".to_string();
                                }
                                let _ = self.config_mgr.save();
                            }
                            let to_resp = ui.add(
                                egui::DragValue::new(&mut self.config_mgr.data.global.test_timeout_secs)
                                    .range(1..=300)
                                    .suffix("s"),
                            ).on_hover_text("测试连接超时（秒），全局设置");
                            if to_resp.changed() { let _ = self.config_mgr.save(); }

                            if ui.button("⬆").on_hover_text("上移").clicked() { do_move_up = true; }
                            if ui.button("⬇").on_hover_text("下移").clicked() { do_move_down = true; }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("  ▶ 启 动  ").clicked() { do_launch = true; }
                            });
                        });
                    });
                }
            }
        });

        // ── 工具管理浮窗 ──────────────────────────────────────────────────────
        {
            let tools_snap = self.config_mgr.data.tools.clone();
            tool_manager_window(
                ctx,
                &mut self.show_tool_mgr,
                &tools_snap,
                &mut self.new_tool_form,
                &mut do_delete_tool,
                &mut do_add_tool,
            );
        }

        // ── 处理动作（所有 panel 借用已释放）────────────────────────────────
        if do_new {
            let tool = self.config_mgr.data.tools.first()
                .map(|t| t.command.clone()).unwrap_or_default();
            // 优先使用当前表单中的目录，回退到配置文件记录的目录
            let dir = self.form.as_ref()
                .map(|f| f.directory.clone())
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| self.config_mgr.data.global.last_directory.clone());
            self.selected_id    = String::new();
            self.pending_delete = false;
            self.form = Some(EditForm {
                key_type: "auth_token".into(),
                tool,
                directory: dir,
                ..Default::default()
            });
            self.status = "新建配置 - 填写后点保存".into();
        }

        if let Some(id) = select_id {
            if let Some(cfg) = self.config_mgr.find_config(&id).cloned() {
                // 优先使用当前表单中的目录（右键打开时已正确设置），回退到配置文件记录的目录
                let dir = self.form.as_ref()
                    .map(|f| f.directory.clone())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| self.config_mgr.data.global.last_directory.clone());
                self.selected_id    = id;
                self.pending_delete = false;
                self.status         = format!("当前：{}", cfg.name);
                self.form           = Some(cfg_to_form(&cfg, &dir));
            }
        }

        if let Some(id) = launch_id {
            if let Some(cfg) = self.config_mgr.find_config(&id).cloned() {
                let dir = self.form.as_ref()
                    .map(|f| f.directory.clone())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| self.config_mgr.data.global.last_directory.clone());
                self.selected_id = id;
                self.form        = Some(cfg_to_form(&cfg, &dir));
                self.launch();
            }
        }

        if do_save           { self.save(); }
        if do_delete_click   { self.pending_delete = true; }
        if do_delete_confirm { self.delete(); }
        if do_delete_cancel  { self.pending_delete = false; }
        if do_test           { self.test_connection(); }
        if do_launch         { self.launch(); }
        if do_toggle_tool_mgr { self.show_tool_mgr = !self.show_tool_mgr; }
        if do_move_up   { self.config_mgr.move_config_up(&self.selected_id.clone()); }
        if do_move_down { self.config_mgr.move_config_down(&self.selected_id.clone()); }
        if let Some(cmd) = do_delete_tool { self.config_mgr.delete_tool(&cmd); }
        if do_add_tool {
            let f = &self.new_tool_form;
            if !f.command.trim().is_empty() && !f.vendor.trim().is_empty() {
                self.config_mgr.add_tool(config::Tool {
                    vendor:         f.vendor.trim().to_string(),
                    command:        f.command.trim().to_string(),
                    env_base_url:   f.env_base_url.trim().to_string(),
                    env_auth_token: f.env_auth_token.trim().to_string(),
                    env_api_key:    f.env_api_key.trim().to_string(),
                    env_proxy:      f.env_proxy.trim().to_string(),
                });
                self.new_tool_form = NewToolForm::default();
                self.status = "工具已添加".into();
            } else {
                self.status = "名称和命令不能为空".into();
            }
        }

        } // ConfigManager
        AppTab::Keychain => {
            self.ui_keychain(ctx);
        }
        } // match active_tab
    }
}

// ── 辅助函数 ──────────────────────────────────────────────────────────────────

/// Grid 表单行：左列标签 + 右列内容
fn form_row(ui: &mut egui::Ui, label: &str, add_content: impl FnOnce(&mut egui::Ui)) {
    ui.label(label);
    ui.horizontal(add_content);
    ui.end_row();
}

fn tool_manager_window(
    ctx: &egui::Context,
    visible: &mut bool,
    tools: &[config::Tool],
    new_form: &mut NewToolForm,
    do_delete: &mut Option<String>,
    do_add: &mut bool,
) {
    if !*visible { return; }
    egui::Window::new("⚙ 工具管理")
        .collapsible(false)
        .resizable(false)
        .min_width(420.0)
        .open(visible)
        .show(ctx, |ui| {
            // 已有工具列表
            egui::Grid::new("tool_list")
                .num_columns(3)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    for t in tools {
                        ui.label(&t.vendor);
                        ui.monospace(&t.command);
                        if ui.small_button("🗑 删除").clicked() {
                            *do_delete = Some(t.command.clone());
                        }
                        ui.end_row();
                    }
                });

            ui.separator();
            ui.label("添加新工具：");
            ui.add_space(2.0);

            egui::Grid::new("new_tool_form")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .min_col_width(90.0)
                .show(ui, |ui| {
                    let w = 240.0;
                    ui.label("名称:"); ui.add(egui::TextEdit::singleline(&mut new_form.vendor).hint_text("如 Moonshot").desired_width(w)); ui.end_row();
                    ui.label("命令:"); ui.add(egui::TextEdit::singleline(&mut new_form.command).hint_text("如 kimi").desired_width(w)); ui.end_row();
                });

            ui.add_space(4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("➕ 添加").clicked() { *do_add = true; }
            });
        });
}

fn cfg_to_form(cfg: &config::Config, last_dir: &str) -> EditForm {
    EditForm {
        id:             cfg.id.clone(),
        name:           cfg.name.clone(),
        description:    cfg.description.clone(),
        tool:           cfg.tool.clone(),
        base_url:       cfg.base_url.clone(),
        key:            cfg.key.clone(),
        key_visible:    false,
        key_type:       cfg.key_type.clone(),
        proxy:          cfg.proxy.clone(),
        command_args:   cfg.command_args.clone(),
        extra_env_text: cfg.extra_env.iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("\n"),
        directory:      last_dir.to_string(),
    }
}

fn form_to_cfg(form: &EditForm) -> config::Config {
    let extra_env: HashMap<String, String> = form.extra_env_text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { return None; }
            let mut it = line.splitn(2, '=');
            let k = it.next()?.trim().to_string();
            let v = it.next().unwrap_or("").trim().to_string();
            (!k.is_empty()).then_some((k, v))
        })
        .collect();

    config::Config {
        id:           form.id.clone(),
        name:         form.name.clone(),
        description:  form.description.clone(),
        tool:         form.tool.clone(),
        base_url:     form.base_url.clone(),
        key:          form.key.clone(),
        key_type:     form.key_type.clone(),
        proxy:        form.proxy.clone(),
        command_args: form.command_args.clone(),
        extra_env,
    }
}

fn resolve_config_path() -> String {
    // 优先级：exe 同目录（便携模式）→ %AppData%/aicode-bat-gui（安装模式）
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| ".".into());

    let local = exe_dir.join("launcher_config.json");
    if local.exists() { return local.to_string_lossy().into_owned(); }

    let app_dir = dirs::config_dir()
        .map(|p| p.join("aicode-bat-gui"))
        .unwrap_or_else(|| exe_dir.clone());
    let _ = std::fs::create_dir_all(&app_dir);
    app_dir.join("launcher_config.json").to_string_lossy().into_owned()
}

fn resolve_keychain_path() -> String {
    // 与 resolve_config_path 相同逻辑，只是文件名不同
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| ".".into());

    // 便携模式：与 launcher_config.json 同目录
    let local_cfg = exe_dir.join("launcher_config.json");
    if local_cfg.exists() {
        return exe_dir.join("keychain.json").to_string_lossy().into_owned();
    }

    let app_dir = dirs::config_dir()
        .map(|p| p.join("aicode-bat-gui"))
        .unwrap_or_else(|| exe_dir.clone());
    let _ = std::fs::create_dir_all(&app_dir);
    app_dir.join("keychain.json").to_string_lossy().into_owned()
}

/// 判断字段是否为敏感字段（密码、密钥等），用于默认遮盖显示
fn is_secret_field(key: &str) -> bool {
    let k = key.to_lowercase();
    k.contains("密码") || k.contains("password") || k.contains("secret")
        || k.contains("token") || k.contains("key")
}

// ── 右键菜单注册 ──────────────────────────────────────────────────────────────
#[cfg(windows)]
fn register_context_menu() -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?;
    let exe_str = exe.to_string_lossy().to_string();
    let exe_dir = exe.parent().ok_or("无法获取 exe 目录")?;
    let dll_path = exe_dir.join("aicode_shell_ext.dll");

    if !dll_path.exists() {
        return Err("未找到 aicode_shell_ext.dll，请确保 DLL 与 exe 在同一目录".into());
    }
    let dll_str = dll_path.to_string_lossy().to_string();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // 清除旧的经典注册方式（command 子键），否则会覆盖 ExplorerCommandHandler
    let _ = hkcu.delete_subkey_all(
        r"Software\Classes\Directory\Background\shell\AICode启动器",
    );

    // 注册 shell verb，设置 ExplorerCommandHandler（Windows 11 现代右键菜单）
    let (key, _) = hkcu
        .create_subkey(r"Software\Classes\Directory\Background\shell\AICode启动器")
        .map_err(|e| e.to_string())?;
    key.set_value(
        "ExplorerCommandHandler",
        &"{A5C7B3F1-2E4D-4A8B-9C1F-3D7E6F8A9B2C}",
    )
    .map_err(|e| e.to_string())?;
    key.set_value("Icon", &exe_str).map_err(|e| e.to_string())?;

    // 注册 CLSID 及 InProcServer32，指向 DLL
    let (srv, _) = hkcu
        .create_subkey(r"Software\Classes\CLSID\{A5C7B3F1-2E4D-4A8B-9C1F-3D7E6F8A9B2C}\InProcServer32")
        .map_err(|e| e.to_string())?;
    srv.set_value("", &dll_str).map_err(|e| e.to_string())?;
    srv.set_value("ThreadingModel", &"Apartment")
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(windows)]
fn unregister_context_menu() -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(
        r"Software\Classes\Directory\Background\shell\AICode启动器",
    );
    let _ = hkcu.delete_subkey_all(
        r"Software\Classes\CLSID\{A5C7B3F1-2E4D-4A8B-9C1F-3D7E6F8A9B2C}",
    );
    Ok(())
}

// ── 数据备份 ────────────────────────────────────────────────────────────────
fn backup_data_files(config_path: &str, keychain_path: &str) -> Result<String, String> {
    use std::path::Path;

    let config_src = Path::new(config_path);
    let data_dir = config_src.parent().ok_or("无法获取数据目录")?;
    let backup_dir = data_dir.join("backup");
    std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;

    let now: std::time::SystemTime = std::time::SystemTime::now();
    let secs = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    // 简易 UTC 时间戳转本地日期时间（东八区 +8h）
    let secs = secs + 8 * 3600;
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    // 从 1970-01-01 算天数 → 年月日
    let (y, mo, d) = days_to_ymd(days);
    let stamp = format!("{:04}{:02}{:02}_{:02}{:02}{:02}", y, mo, d, h, m, s);
    let mut count = 0;

    for src_path in [config_path, keychain_path] {
        let src = Path::new(src_path);
        if !src.exists() {
            continue;
        }
        let name = src.file_name().ok_or("无效文件名")?;
        let dest = backup_dir.join(format!("{}_{}", stamp, name.to_string_lossy()));
        std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
        count += 1;
    }

    Ok(format!("已备份 {} 个文件到 backup/", count))
}

/// 从 Unix epoch 天数计算 (年, 月, 日)
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut y = 1970;
    loop {
        let ydays = if is_leap(y) { 366 } else { 365 };
        if days < ydays { break; }
        days -= ydays;
        y += 1;
    }
    let mdays: [u64; 12] = if is_leap(y) {
        [31,29,31,30,31,30,31,31,30,31,30,31]
    } else {
        [31,28,31,30,31,30,31,31,30,31,30,31]
    };
    let mut mo = 0;
    for &md in &mdays {
        if days < md { break; }
        days -= md;
        mo += 1;
    }
    (y, mo + 1, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
