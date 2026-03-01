#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api_test;
mod config;
mod launcher;

use eframe::egui;
use std::collections::HashMap;
use std::sync::mpsc;

// ── 入口 ─────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CLI 启动管理器")
            .with_inner_size([860.0, 600.0])
            .with_min_inner_size([640.0, 420.0]),
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

struct LauncherApp {
    config_mgr:     config::ConfigManager,
    selected_id:    String,
    form:           Option<EditForm>,
    status:         String,
    dark_mode:      bool,
    pending_delete: bool,
    test_rx:        Option<mpsc::Receiver<Result<String, String>>>,
    show_tool_mgr:  bool,
    new_tool_form:  NewToolForm,
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

// ── 初始化 ────────────────────────────────────────────────────────────────────

impl LauncherApp {
    fn new() -> Self {
        let config_mgr = config::ConfigManager::new(resolve_config_path());

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
            config_mgr,
            selected_id,
            form,
            status:         "就绪".into(),
            dark_mode:      false,
            pending_delete: false,
            test_rx:        None,
            show_tool_mgr:  false,
            new_tool_form:  NewToolForm::default(),
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
}

// ── UI ────────────────────────────────────────────────────────────────────────

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        // ── 顶栏 ──────────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("CLI 启动管理器");
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
                                let vars = [
                                    ("ANTHROPIC_MODEL",                  "Claude - 覆盖默认模型"),
                                    ("ANTHROPIC_DEFAULT_OPUS_MODEL",     "Claude - 映射 Opus 到指定模型"),
                                    ("ANTHROPIC_DEFAULT_SONNET_MODEL",   "Claude - 映射 Sonnet 到指定模型"),
                                    ("ANTHROPIC_DEFAULT_HAIKU_MODEL",    "Claude - 映射 Haiku 到指定模型"),
                                    ("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",  "Claude - 上下文压缩触发百分比（0-100）"),
                                    ("OPENAI_MODEL",                     "Qwen/OpenAI 兼容 - 覆盖默认模型"),
                                    ("GEMINI_MODEL",                     "Gemini - 覆盖默认模型"),
                                ];
                                egui::Grid::new("env_help").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
                                    for (k, desc) in &vars {
                                        ui.add(egui::Label::new(
                                            egui::RichText::new(*k).monospace().weak()
                                        ));
                                        ui.label(*desc);
                                        ui.end_row();
                                    }
                                });
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
            let dir  = self.config_mgr.data.global.last_directory.clone();
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
                let dir = self.config_mgr.data.global.last_directory.clone();
                self.selected_id    = id;
                self.pending_delete = false;
                self.status         = format!("当前：{}", cfg.name);
                self.form           = Some(cfg_to_form(&cfg, &dir));
            }
        }

        if let Some(id) = launch_id {
            if let Some(cfg) = self.config_mgr.find_config(&id).cloned() {
                let dir = self.config_mgr.data.global.last_directory.clone();
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

// ── 右键菜单注册 ──────────────────────────────────────────────────────────────
#[cfg(windows)]
fn register_context_menu() -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .to_string();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\Classes\Directory\Background\shell\AICode启动器")
        .map_err(|e| e.to_string())?;
    key.set_value("", &"AICode 启动器").map_err(|e| e.to_string())?;
    key.set_value("Icon", &exe).map_err(|e| e.to_string())?;

    let (cmd, _) = hkcu
        .create_subkey(r"Software\Classes\Directory\Background\shell\AICode启动器\command")
        .map_err(|e| e.to_string())?;
    let cmd_val = format!("\"{}\" \"%V\"", exe);
    cmd.set_value("", &cmd_val).map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(windows)]
fn unregister_context_menu() -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.delete_subkey_all(r"Software\Classes\Directory\Background\shell\AICode启动器")
        .map_err(|e| e.to_string())
}
