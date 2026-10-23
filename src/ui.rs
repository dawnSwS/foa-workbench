use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui::{self, RichText};

use crate::model::{BankReport, FieldKind, FIELDS};
use crate::scan::{self, ScanResult};
use crate::{bank, edit_policy, paths, replay, theme};

#[path = "replay_ui.rs"]
mod replay_ui;

enum LoadedData {
    Banks(ScanResult),
    Replay(Box<replay::ReplayReport>),
}

type ScanMessage = Result<LoadedData, String>;

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
enum Tab {
    #[default]
    Overview,
    Fields,
    RoleBans,
}

#[derive(Default)]
pub struct Viewer {
    root: String,
    target: String,
    data: ScanResult,
    replay: Option<replay::ReplayReport>,
    replay_player: Option<usize>,
    replay_page: replay_ui::Page,
    account: Option<usize>,
    report: Option<usize>,
    tab: Tab,
    filter: String,
    group: Option<FieldKind>,
    worker: Option<Receiver<ScanMessage>>,
    status: String,
    error: Option<String>,
    show_issues: bool,
    show_home: bool,
    edit: Option<EditDraft>,
    confirmation: Option<Confirmation>,
    allow_close: bool,
}

impl Viewer {
    pub fn new(context: &egui::Context) -> Self {
        let default = paths::default_root();
        let mut viewer = Self {
            root: default
                .as_ref()
                .map_or_else(String::new, |path| path.display().to_string()),
            target: "RR.SC2Bank".into(),
            show_home: true,
            status: "准备就绪 · 支持拖入录像、存档或目录".into(),
            ..Self::default()
        };
        if let Some(path) = default.filter(|path| path.is_dir()) {
            viewer.start(path, context);
        }
        viewer
    }

    fn start(&mut self, root: PathBuf, context: &egui::Context) {
        if self.edit.is_some() {
            self.error = Some("打开其他存档前需保存或放弃当前修改。".into());
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let target = self.target.clone();
        let repaint = context.clone();
        let task = std::thread::Builder::new()
            .name("bank-reader".into())
            .spawn(move || {
                let result = if root
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("SC2Replay"))
                {
                    replay::parse_file(&root)
                        .map(Box::new)
                        .map(LoadedData::Replay)
                } else {
                    scan::run(&root, &target).map(LoadedData::Banks)
                };
                let _ = sender.send(result);
                repaint.request_repaint();
            });
        match task {
            Ok(_) => {
                self.worker = Some(receiver);
                self.data = ScanResult::default();
                self.replay = None;
                self.replay_player = None;
                self.replay_page = replay_ui::Page::default();
                self.account = None;
                self.report = None;
                self.error = None;
                self.show_home = false;
                self.filter.clear();
                self.group = None;
                self.status = "正在读取存档或录像…".into();
            }
            Err(error) => self.error = Some(format!("无法启动扫描：{error}")),
        }
    }

    fn scan_input(&mut self, context: &egui::Context) {
        match paths::expand_input(&self.root) {
            Ok(path) => self.start(path, context),
            Err(error) => self.error = Some(error),
        }
    }

    fn poll(&mut self) {
        let message = self
            .worker
            .as_ref()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("扫描任务异常结束，需重新扫描。".into()))
                }
            });
        let Some(message) = message else { return };
        self.worker = None;
        match message {
            Ok(LoadedData::Banks(data)) => {
                self.show_home = data.reports.is_empty();
                self.status = format!(
                    "{} 个账号  /  {} 份可读存档  /  {} 个问题",
                    data.accounts.len(),
                    data.reports.len(),
                    data.issues.len()
                );
                self.report = (!data.reports.is_empty()).then_some(0);
                self.account = self
                    .report
                    .and_then(|index| data.reports.get(index))
                    .and_then(|report| {
                        data.accounts.iter().position(|account| {
                            account.banks.iter().any(|bank| bank.path == report.path)
                        })
                    })
                    .or_else(|| (!data.accounts.is_empty()).then_some(0));
                self.data = data;
            }
            Ok(LoadedData::Replay(data)) => {
                self.status = format!(
                    "录像 · {} 名玩家 / {} 次 Bank 同步 / {} 个提示",
                    data.players.len(),
                    data.players
                        .iter()
                        .map(|player| player.banks.len())
                        .sum::<usize>(),
                    data.warnings.len()
                );
                self.replay = Some(*data);
            }
            Err(error) => {
                self.show_home = true;
                self.status = "扫描未完成".into();
                self.error = Some(error);
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        egui::Panel::top("header")
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::vec2(22.0, 12.0)),
            )
            .show(ui, |ui| {
                theme::compact_controls(ui);
                ui.horizontal(|ui| {
                    ui.strong(if self.worker.is_some() {
                        "正在读取文件"
                    } else if self.show_home {
                        "首页"
                    } else if self.replay.is_some() {
                        "录像工作区"
                    } else {
                        "存档工作区"
                    });
                    if self.edit.is_some() {
                        theme::status_badge(ui, "正在修改", theme::AMBER);
                    } else if self.replay.is_some() && !self.show_home {
                        theme::badge(ui, "只读分析");
                    }
                    if !self.show_home && ui.available_width() > 420.0 {
                        if let Some(path) = self.current_path() {
                            let filename = path.file_name().map_or_else(String::new, |name| {
                                name.to_string_lossy().into_owned()
                            });
                            ui.add_sized(
                                [ui.available_width() - 220.0, 24.0],
                                egui::Label::new(theme::muted(filename).size(12.0)).truncate(),
                            )
                            .on_hover_text(path.display().to_string());
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button("显示", |ui| {
                            for (scale, label) in [
                                (1.0, "100% · 标准"),
                                (1.15, "115% · 放大"),
                                (1.3, "130% · 更大"),
                            ] {
                                if ui
                                    .selectable_label(
                                        (context.zoom_factor() - scale).abs() < 0.01,
                                        label,
                                    )
                                    .clicked()
                                {
                                    context.set_zoom_factor(scale);
                                    ui.close();
                                }
                            }
                        });
                        ui.add_enabled_ui(self.edit.is_none() && self.worker.is_none(), |ui| {
                            ui.menu_button("打开文件…", |ui| self.open_menu(ui, &context));
                        });
                    });
                });
            });
    }

    fn current_path(&self) -> Option<&Path> {
        self.replay
            .as_ref()
            .map(|replay| replay.path.as_path())
            .or_else(|| {
                self.report
                    .and_then(|index| self.data.reports.get(index))
                    .map(|report| report.path.as_path())
            })
    }

    fn open_menu(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        for (label, extension) in [("分析录像…", "SC2Replay"), ("查看存档…", "SC2Bank")]
        {
            if ui.button(label).clicked() {
                ui.close();
                self.pick_file(extension, context);
            }
        }
        if ui.button("扫描存档目录…").clicked() {
            ui.close();
            self.pick_folder(context);
        }
        if ui.button("输入路径与扫描设置").clicked() {
            self.show_home = true;
            ui.close();
        }
    }

    fn pick_file(&mut self, extension: &str, context: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().add_filter(extension, &[extension]);
        if extension.eq_ignore_ascii_case("SC2Replay") {
            let current = paths::expand_input(&self.root).ok();
            let default = paths::default_root();
            if let Some(directory) =
                paths::replay_start_directory(current.as_deref(), default.as_deref())
            {
                dialog = dialog.set_directory(directory);
            }
        }
        if let Some(path) = dialog.pick_file() {
            self.root = path.display().to_string();
            self.start(path, context);
        }
    }

    fn pick_folder(&mut self, context: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.root = path.display().to_string();
            self.start(path, context);
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        ui.add_enabled_ui(self.worker.is_none() && self.edit.is_none(), |ui| {
            ui.horizontal(|ui| {
                let input = ui.add(
                    egui::TextEdit::singleline(&mut self.root)
                        .desired_width((ui.available_width() - 120.0).max(120.0))
                        .hint_text("文件或目录路径"),
                );
                let enter =
                    input.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ui.add(theme::primary_button("读取路径")).clicked() || enter {
                    self.scan_input(context);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("扫描文件名");
                ui.add(egui::TextEdit::singleline(&mut self.target).desired_width(150.0));
                if ui.button("扫描默认位置").clicked() {
                    if let Some(path) = paths::default_root() {
                        self.root = path.display().to_string();
                        self.start(path, context);
                    } else {
                        self.error = Some("未找到默认存档位置，需手动指定 Accounts 目录。".into());
                    }
                }
            });
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::vec2(22.0, 5.0)),
            )
            .show(ui, |ui| {
                theme::compact_controls(ui);
                ui.horizontal(|ui| {
                    if self.worker.is_some() {
                        ui.spinner();
                    }
                    if !self.data.issues.is_empty()
                        && ui
                            .small_button(format!("读取提示 · {}", self.data.issues.len()))
                            .clicked()
                    {
                        self.show_issues = true;
                    }
                    let (message, color) = self
                        .error
                        .as_ref()
                        .map_or((&self.status, theme::MUTED), |error| (error, theme::DANGER));
                    ui.add(
                        egui::Label::new(RichText::new(message).color(color).size(12.0)).truncate(),
                    )
                    .on_hover_text(message);
                });
            });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("workspace_navigation")
            .exact_size(204.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(theme::NAV_BG).inner_margin(16.0))
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    theme::emblem(ui);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("缄默之陨")
                                .size(19.0)
                                .strong()
                                .color(theme::NAV_TEXT),
                        );
                        ui.label(
                            RichText::new("数据工作台")
                                .size(11.0)
                                .color(theme::NAV_MUTED),
                        );
                    });
                });
                ui.add_space(24.0);
                egui::ScrollArea::vertical()
                    .id_salt("workspace_navigation_scroll")
                    .max_height((ui.available_height() - 54.0).max(40.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.navigation(ui));
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(if edit_policy::ROLE_BAN_ONLY {
                            "普通版 · 角色限制与改名"
                        } else {
                            "完整版 · 存档与录像"
                        })
                        .size(11.0)
                        .color(theme::NAV_MUTED),
                    );
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                            .size(11.0)
                            .color(theme::NAV_MUTED),
                    );
                });
            });
    }

    fn navigation(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(self.edit.is_none(), |ui| {
            if theme::nav_item(ui, self.show_home, "首页", "打开录像或存档").clicked() {
                self.show_home = true;
            }
        });
        if self.replay.is_some() || !self.data.accounts.is_empty() {
            ui.add_space(22.0);
            ui.label(RichText::new("当前文件").size(11.0).color(theme::NAV_MUTED));
            if self.replay.is_some() {
                if replay_ui::navigation(
                    ui,
                    &mut self.replay_page,
                    &mut self.replay_player,
                    !self.show_home,
                ) {
                    self.show_home = false;
                }
            } else {
                self.bank_navigation(ui);
            }
        }
    }

    fn bank_navigation(&mut self, ui: &mut egui::Ui) {
        for (tab, title, caption) in [
            (Tab::Overview, "存档总览", "战绩与阵营评分"),
            (Tab::RoleBans, "角色限制", "查看与调整角色 Ban"),
            (Tab::Fields, "全部字段", "搜索统计与标记"),
        ] {
            if theme::nav_item(ui, !self.show_home && self.tab == tab, title, caption).clicked() {
                self.tab = tab;
                self.show_home = false;
                self.filter.clear();
                self.group = None;
            }
        }
        ui.add_space(22.0);
        ui.label(RichText::new("切换存档").size(11.0).color(theme::NAV_MUTED));
        ui.add_enabled_ui(self.edit.is_none(), |ui| self.bank_picker(ui));
        if self.edit.is_some() {
            ui.label(
                RichText::new("保存或退出修改后可切换文件")
                    .size(11.0)
                    .color(theme::NAV_MUTED),
            );
        }
    }

    fn bank_picker(&mut self, ui: &mut egui::Ui) {
        let mut selected_account = self.account;
        let account_label = self
            .account
            .and_then(|index| self.data.accounts.get(index))
            .map_or("选择账号", |account| account.id.as_str());
        egui::ComboBox::from_id_salt("bank_account")
            .width(ui.available_width())
            .truncate()
            .selected_text(short_label(account_label, 18))
            .show_ui(ui, |ui| {
                for (index, account) in self.data.accounts.iter().enumerate() {
                    ui.selectable_value(&mut selected_account, Some(index), &account.id)
                        .on_hover_text(account.path.display().to_string());
                }
            })
            .response
            .on_hover_text(account_label);
        if let Some(index) = selected_account.filter(|value| Some(*value) != self.account) {
            self.select_account(index);
        }
        let mut chosen = None;
        if let Some(account) = self.account.and_then(|index| self.data.accounts.get(index)) {
            if account.banks.is_empty() {
                ui.label(
                    RichText::new("此账号下未找到目标存档")
                        .size(12.0)
                        .color(theme::NAV_MUTED),
                );
                for server in &account.servers {
                    ui.label(
                        RichText::new(scan::server_name(Some(*server)))
                            .size(12.0)
                            .color(theme::NAV_MUTED),
                    );
                }
            }
            egui::ScrollArea::vertical()
                .id_salt("bank_files")
                .max_height((ui.available_height() - 70.0).clamp(60.0, 220.0))
                .show(ui, |ui| {
                    for file in &account.banks {
                        let index = self
                            .data
                            .reports
                            .iter()
                            .position(|report| report.path == file.path);
                        let filename = file
                            .path
                            .file_name()
                            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                        ui.add_enabled_ui(index.is_some(), |ui| {
                            if theme::nav_item(
                                ui,
                                self.report == index && index.is_some(),
                                &scan::server_name(file.server_id),
                                &short_label(&filename, 12),
                            )
                            .on_hover_text(file.path.display().to_string())
                            .clicked()
                            {
                                chosen = index;
                            }
                        });
                    }
                });
        }
        if let Some(index) = chosen {
            self.report = Some(index);
            self.show_home = false;
            self.filter.clear();
            self.group = None;
        }
    }

    fn home(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        egui::ScrollArea::vertical()
            .id_salt("home_content")
            .show(ui, |ui| {
                ui.add_space(22.0);
                theme::section_heading(
                    ui,
                    "存档与录像分析",
                    "录像玩家数据、异形选人复盘及本地存档管理。",
                );
                if let Some(error) = &self.error {
                    ui.label(RichText::new(error).color(theme::DANGER));
                } else if !self.data.accounts.is_empty() && self.data.reports.is_empty() {
                    ui.label(RichText::new("已检测到账号，未找到可读存档。扫描范围由目录和文件名决定，解析问题见读取提示。").color(theme::AMBER));
                }
                ui.add_space(24.0);
                let cards = [
                    (
                        "01 / 录像分析",
                        "玩家数据与选人复盘",
                        "全体玩家数据、存档快照与异形选人依据。",
                        "分析录像",
                        "SC2Replay",
                    ),
                    (
                        "02 / 存档管理",
                        "本地存档查看与修改",
                        "累计战绩、角色限制与局内名字编辑。",
                        "打开存档",
                        "SC2Bank",
                    ),
                ];
                let columns = if ui.available_width() >= 660.0 { 2 } else { 1 };
                for row in cards.chunks(columns) {
                    ui.columns(columns, |uis| {
                        for (column, (eyebrow, title, description, button, extension)) in
                            uis.iter_mut().zip(row)
                        {
                            theme::panel().show(column, |ui| {
                                ui.set_min_width((ui.available_width() - 1.0).max(0.0));
                                ui.label(
                                    RichText::new(*eyebrow)
                                        .color(theme::ACCENT)
                                        .size(12.0)
                                        .strong(),
                                );
                                ui.add_space(14.0);
                                ui.label(RichText::new(*title).size(21.0).strong());
                                ui.label(theme::muted(*description));
                                ui.add_space(22.0);
                                if ui
                                    .add_enabled(
                                        self.worker.is_none(),
                                        theme::primary_button(*button),
                                    )
                                    .clicked()
                                {
                                    self.pick_file(extension, context);
                                }
                            });
                        }
                    });
                    ui.add_space(14.0);
                }
                theme::panel().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong("扫描本地存档");
                        ui.label(theme::muted("扫描 Accounts 目录中的账号、区服和存档文件。"));
                        if ui
                            .add_enabled(self.worker.is_none(), egui::Button::new("选择存档目录…"))
                            .clicked()
                        {
                            self.pick_folder(context);
                        }
                    });
                });
                ui.add_space(8.0);
                ui.collapsing("手动路径与扫描设置", |ui| {
                    self.controls(ui, context);
                });
                ui.add_space(18.0);
                ui.label(theme::muted("支持拖入录像、存档文件及目录。").size(13.0));
            });
    }

    fn select_account(&mut self, index: usize) {
        self.account = Some(index);
        self.show_home = false;
        self.filter.clear();
        self.group = None;
        self.report = self.data.accounts.get(index).and_then(|account| {
            self.data
                .reports
                .iter()
                .position(|report| account.banks.iter().any(|file| file.path == report.path))
        });
    }

    fn issue_window(&mut self, context: &egui::Context) {
        egui::Window::new("扫描与解析详情")
            .open(&mut self.show_issues)
            .default_width(650.0)
            .show(context, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(450.0)
                    .show(ui, |ui| {
                        for issue in &self.data.issues {
                            theme::panel().show(ui, |ui| {
                                ui.label(RichText::new(issue).color(theme::AMBER));
                            });
                            ui.add_space(6.0);
                        }
                    });
            });
    }
}

impl Viewer {
    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        self.poll();
        if self.worker.is_some() {
            context.request_repaint_after(Duration::from_millis(100));
        }
        if !self.allow_close
            && context.input(|input| input.viewport().close_requested())
            && self.has_unsaved_changes()
        {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirmation = Some(Confirmation::Close);
        }
        if self.worker.is_none() {
            let dropped = context.input(|input| {
                input
                    .raw
                    .dropped_files
                    .first()
                    .map(|file| file.path().to_path_buf())
            });
            if let Some(path) = dropped {
                if self.edit.is_some() {
                    self.error = Some("打开拖入的文件前需保存或放弃当前修改。".into());
                } else {
                    self.root = path.display().to_string();
                    self.start(path, &context);
                }
            }
        }
        self.sidebar(ui);
        self.header(ui);
        self.footer(ui);
        let mut action = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BACKGROUND).inner_margin(24.0))
            .show(ui, |ui| {
                if self.show_home && self.edit.is_none() {
                    self.home(ui, &context);
                } else if let Some(replay) = self.replay.as_mut() {
                    replay_ui::view(
                        ui,
                        replay,
                        &mut self.replay_page,
                        &mut self.replay_player,
                        &mut self.tab,
                        &mut self.filter,
                        &mut self.group,
                    );
                } else if let Some(report) =
                    self.report.and_then(|index| self.data.reports.get(index))
                {
                    action = report_toolbar(ui, report, self.tab, self.edit.as_ref());
                    egui::ScrollArea::vertical()
                        .id_salt((
                            "content_scroll",
                            &report.path,
                            self.tab,
                            self.edit.is_some(),
                        ))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            report_view(
                                ui,
                                report,
                                self.tab,
                                &mut self.filter,
                                &mut self.group,
                                self.edit.as_mut(),
                            );
                        });
                } else if self.worker.is_some() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(100.0);
                        ui.spinner();
                        ui.heading("正在读取文件");
                        ui.label(theme::muted("读取完成后自动显示文件内容。"));
                    });
                } else {
                    self.home(ui, &context);
                }
            });
        if let Some(action) = action {
            self.edit_action(action);
        }
        self.issue_window(&context);
        self.edit_dialogs(&context);
    }
}

impl eframe::App for Viewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

fn short_label(text: &str, limit: usize) -> String {
    let mut result: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        result.push('…');
    }
    result
}

fn report_heading(ui: &mut egui::Ui, report: &BankReport) {
    let name = report
        .player_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or("未设置局内名字");
    ui.add(egui::Label::new(RichText::new(name).size(26.0).strong()).truncate())
        .on_hover_text(name);
    ui.horizontal_wrapped(|ui| {
        theme::badge(ui, &scan::server_name(report.server_id));
        ui.label(
            theme::muted(format!(
                "账号 {} · 玩家 ID {}",
                report.account_id,
                metric(report, "player_id")
            ))
            .size(12.0),
        );
        ui.label(theme::muted(format!("Bank v{}", report.bank_version)).size(12.0));
    });
}

fn report_toolbar(
    ui: &mut egui::Ui,
    report: &BankReport,
    tab: Tab,
    draft: Option<&EditDraft>,
) -> Option<EditAction> {
    report_heading(ui, report);
    ui.add_space(8.0);
    let mut action = None;
    theme::panel()
        .inner_margin(egui::vec2(12.0, 8.0))
        .show(ui, |ui| {
            theme::compact_controls(ui);
            ui.horizontal_wrapped(|ui| {
                action = edit_toolbar(ui, report, draft);
                ui.menu_button("修改说明", |ui| edit_help(ui, report));
            });
            if let Some(draft) = draft {
                edit_notice(ui, report, draft);
            }
        });
    ui.add_space(12.0);
    let (title, subtitle) = match tab {
        Tab::Overview => ("存档总览", "当前存档的累计记录"),
        Tab::RoleBans => ("角色限制", "是表示排除该角色，否表示取消限制"),
        Tab::Fields => ("全部字段", "搜索名称或字段键，按类型筛选"),
    };
    ui.horizontal_wrapped(|ui| {
        ui.strong(title);
        ui.label(theme::muted(subtitle).size(12.0));
    });
    ui.add_space(8.0);
    action
}

fn report_view(
    ui: &mut egui::Ui,
    report: &BankReport,
    tab: Tab,
    filter: &mut String,
    group: &mut Option<FieldKind>,
    mut draft: Option<&mut EditDraft>,
) {
    if let Some(draft) = draft.as_deref_mut() {
        edit_player_name(ui, draft);
    }
    match tab {
        Tab::Overview => overview(ui, report),
        Tab::Fields => fields_view(ui, report, filter, group, draft, false),
        Tab::RoleBans => fields_view(ui, report, filter, group, draft, true),
    }
    ui.add_space(18.0);
    ui.collapsing("文件位置与读取详情", |ui| {
        ui.label(theme::muted(report.path.display().to_string()).size(13.0));
        ui.label(theme::muted(&report.decode_note).size(13.0));
        let signature = if report.signature_present {
            "文件含 SC2 签名；未验证引擎签名。修改后会移除失效签名。"
        } else {
            "文件未包含 SC2 签名。"
        };
        ui.label(theme::muted(signature).size(13.0));
    });
}

fn edit_player_name(ui: &mut egui::Ui, draft: &mut EditDraft) {
    theme::panel().show(ui, |ui| {
        ui.checkbox(&mut draft.change_name, "同时修改局内名字");
        ui.add_enabled_ui(draft.change_name, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("局内名字");
                ui.add(egui::TextEdit::singleline(&mut draft.player_name).desired_width(280.0));
            });
        });
        if let Some(error) = draft.name_error() {
            ui.label(
                RichText::new(format!(
                    "{error} 将跳过名字修改，保留存档原名；其他修改照常保存。"
                ))
                .color(theme::AMBER),
            );
        }
        player_name_help(ui);
    });
    ui.add_space(10.0);
}

fn player_name_help(ui: &mut egui::Ui) {
    for hint in [
        "此处修改存档中的局内名字，长度须为 3–17 个 UTF-8 字节；长度不符时跳过名字修改，保留存档原名，其他修改照常保存。",
        "常用汉字通常占 3 字节，英文字母、数字及普通空格各占 1 字节；例如“黎”=3 字节、“黎明”=6 字节。纯常用汉字可用 1–5 个，混合名字按总字节数计算。",
        "使用自定义名需在大厅选择“自定义”且房间允许；还需通过敏感词和名称禁用检查。",
        "游戏内 !Name 命令另限 4–16 字节，与此处的存档读取规则不同。",
    ] {
        ui.label(theme::muted(hint));
    }
}

fn metric(report: &BankReport, key: &str) -> String {
    report
        .metric(key)
        .map_or_else(|| "—".into(), |number| number.to_string())
}

fn win_rate(report: &BankReport) -> String {
    let values = report
        .metric("lv_statsGamesPlayed")
        .zip(report.metric("lv_statsGamesWon"));
    if let Some((played, won)) = values {
        if let (Ok(played), Ok(won)) = (u32::try_from(played), u32::try_from(won)) {
            if played > 0 {
                return format!("{:.1}%", f64::from(won) / f64::from(played) * 100.0);
            }
        }
    }
    "—".into()
}

fn overview(ui: &mut egui::Ui, report: &BankReport) {
    let cards = [
        (
            "游戏次数",
            metric(report, "lv_statsGamesPlayed"),
            "总游戏记录",
        ),
        ("游戏胜利", metric(report, "lv_statsGamesWon"), "所有阵营"),
        ("累计胜率", win_rate(report), "胜场 / 总场次"),
        (
            "HMMR",
            metric(report, "lv_alienSelectionHMMR"),
            "阵营评分",
        ),
        (
            "AMMR",
            metric(report, "lv_alienSelectionAMMR"),
            "阵营评分",
        ),
        (
            "异形被杀",
            metric(report, "lv_statsAliensKilled"),
            "累计击杀记录",
        ),
    ];
    let column_count = if ui.available_width() >= 760.0 { 3 } else { 2 };
    for row in cards.chunks(column_count) {
        ui.columns(column_count, |columns| {
            for (column, (label, value, caption)) in columns.iter_mut().zip(row) {
                theme::stat(column, label, value, caption);
            }
        });
        ui.add_space(8.0);
    }
    ui.add_space(12.0);
    ui.columns(2, |columns| {
        for (column, (title, rows)) in columns.iter_mut().zip([
            (
                "阵营战绩",
                [
                    ("人类游戏胜利", "lv_statsGamesWonHuman"),
                    ("异形子代赢得游戏", "lv_statsGamesWonSpawn"),
                    ("游戏获胜异形主宰", "lv_statsGamesWonHost"),
                    ("游戏胜利指导", "lv_statsGamesWonAdvisor"),
                ],
            ),
            (
                "其他记录",
                [
                    ("秒退次数", "lv_statsGamesLeft"),
                    ("游戏失败", "lv_statsGamesLost"),
                    ("队友杀手", "lv_statsTeamkills"),
                    ("玩家子代", "lv_statsPlayersSpawned"),
                ],
            ),
        ]) {
            let width = (column.available_width() - 32.0).max(0.0);
            theme::panel().show(column, |ui| {
                ui.set_min_width(width);
                ui.label(RichText::new(title).size(17.0).strong());
                ui.add_space(10.0);
                for (label, key) in rows {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::Label::new(theme::muted(label)).wrap())
                            .on_hover_text(key);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(metric(report, key))
                                    .color(theme::TEXT)
                                    .strong()
                                    .size(17.0),
                            );
                        });
                    });
                }
            });
        }
    });
}

fn fields_view(
    ui: &mut egui::Ui,
    report: &BankReport,
    filter: &mut String,
    group: &mut Option<FieldKind>,
    draft: Option<&mut EditDraft>,
    role_bans_only: bool,
) {
    theme::panel().inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        field_filters(ui, filter, group, role_bans_only);
        let query = filter.trim().to_lowercase();
        let visible: Vec<_> = FIELDS
            .iter()
            .filter_map(|field| {
                let value = report.fields.get(field.key)?;
                ((if role_bans_only {
                    edit_policy::is_role_ban(field.key)
                } else {
                    group.is_none_or(|kind| kind == field.kind)
                }) && (field.label.contains(&query) || field.key.to_lowercase().contains(&query)))
                .then_some((field, *value))
            })
            .collect();
        if role_bans_only {
            ui.label(theme::muted("普通多人开局中，角色 1–10 的限制有效；列兵（11）与预留位（12）不受此项限制影响。").size(12.0));
            role_cards(ui, &visible, draft);
        } else {
            fields_grid(ui, report.bank_version, &visible, draft);
        }
        if visible.is_empty() {
            ui.add_space(16.0);
            ui.label(theme::muted(if role_bans_only && report.bank_version < 3 {
                "Bank v0–v2 不含角色 Ban 位，无法设置角色排除。"
            } else {
                "无符合当前搜索及筛选条件的字段。"
            }));
        }
        ui.label(
            theme::muted(format!(
                "显示 {} / {} 项字段",
                visible.len(),
                report.fields.len()
            ))
            .size(13.0),
        );
    });
}

fn role_cards(
    ui: &mut egui::Ui,
    fields: &[(&crate::model::Field, u64)],
    mut draft: Option<&mut EditDraft>,
) {
    let count = if ui.available_width() >= 780.0 { 3 } else { 2 };
    for row in fields.chunks(count) {
        ui.columns(count, |columns| {
            for (column, (field, value)) in columns.iter_mut().zip(row) {
                theme::panel().inner_margin(12.0).show(column, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(field.label).strong()).truncate())
                        .on_hover_text(format!("{}\n{}", field.label, field.key));
                    let editable = draft
                        .as_deref_mut()
                        .and_then(|draft| draft.values.get_mut(field.key));
                    if let Some(input) = editable {
                        let mut excluded = input == "1";
                        if ui.checkbox(&mut excluded, "排除此角色").changed() {
                            *input = u64::from(excluded).to_string();
                        }
                        if parse_input(input, 1).ok() == Some(*value) {
                            ui.label(theme::muted("未修改").size(12.0));
                        } else {
                            ui.label(RichText::new("待保存").color(theme::AMBER).size(12.0));
                        }
                    } else {
                        theme::status_badge(
                            ui,
                            if *value == 0 {
                                "未设置排除"
                            } else {
                                "已设置排除"
                            },
                            if *value == 0 {
                                theme::MUTED
                            } else {
                                theme::AMBER
                            },
                        );
                        ui.label(theme::muted(format!("存档标记 · {value}")).size(12.0));
                    }
                });
            }
        });
        ui.add_space(6.0);
    }
}

fn field_filters(
    ui: &mut egui::Ui,
    filter: &mut String,
    group: &mut Option<FieldKind>,
    role_bans_only: bool,
) {
    let categories = [
        (None, "全部"),
        (Some(FieldKind::Statistic), "统计"),
        (Some(FieldKind::Flag), "标记"),
        (Some(FieldKind::Role), "角色"),
        (Some(FieldKind::Identity), "身份"),
    ];
    ui.scope(|ui| {
        theme::compact_controls(ui);
        let width = ui.available_width();
        let wide = width >= 800.0 && !role_bans_only;
        let reserved = if role_bans_only {
            72.0
        } else if wide {
            370.0
        } else {
            172.0
        };
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(filter)
                    .hint_text(theme::muted("搜索名称或原始字段…"))
                    .desired_width((width - reserved).max(100.0)),
            );
            if wide {
                for (value, label) in categories {
                    ui.selectable_value(group, value, label);
                }
            } else if !role_bans_only {
                let selected = categories.iter().find(|(value, _)| value == group);
                egui::ComboBox::from_id_salt("field_category")
                    .selected_text(selected.map_or("全部", |(_, label)| *label))
                    .width(76.0)
                    .show_ui(ui, |ui| {
                        for (value, label) in categories {
                            ui.selectable_value(group, value, label);
                        }
                    });
            }
            if ui
                .add_enabled(
                    !filter.is_empty() || group.is_some(),
                    egui::Button::new("重置"),
                )
                .clicked()
            {
                filter.clear();
                *group = None;
            }
        });
    });
    if !role_bans_only {
        let id = egui::Id::unique("show_field_keys");
        let mut show = ui
            .ctx()
            .data_mut(|data| data.get_temp::<bool>(id).unwrap_or(false));
        if ui.checkbox(&mut show, "显示原始字段键").changed() {
            ui.ctx().data_mut(|data| data.insert_temp(id, show));
        }
    }
}

fn field_cell(ui: &mut egui::Ui, width: f32, text: RichText, truncate: bool) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 42.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            let label = egui::Label::new(text);
            ui.add(if truncate {
                label.truncate()
            } else {
                label.wrap()
            })
        },
    )
    .inner
}

fn fields_grid(
    ui: &mut egui::Ui,
    version: u32,
    fields: &[(&crate::model::Field, u64)],
    mut draft: Option<&mut EditDraft>,
) {
    let show_keys = ui.ctx().data_mut(|data| {
        data.get_temp::<bool>(egui::Id::unique("show_field_keys"))
            .unwrap_or(false)
    });
    let compact = !show_keys || ui.available_width() < 650.0;
    let width = ui.available_width().max(320.0);
    let value_width = if draft.is_some() { 174.0 } else { 112.0 };
    let name_width = if compact {
        width - value_width - 16.0
    } else {
        (width * 0.29).clamp(170.0, 260.0)
    };
    let key_width = (width - name_width - value_width - 32.0).max(210.0);
    egui::ScrollArea::horizontal()
        .id_salt("field_columns")
        .show(ui, |ui| {
            egui::Grid::new("fields")
                .num_columns(if compact { 2 } else { 3 })
                .striped(true)
                .min_row_height(42.0)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    field_cell(
                        ui,
                        name_width,
                        RichText::new(if compact {
                            "名称 · 悬停查看原字段"
                        } else {
                            "名称"
                        })
                        .size(if compact { 13.0 } else { 16.0 })
                        .strong(),
                        false,
                    );
                    if !compact {
                        field_cell(ui, key_width, theme::muted("原始字段").strong(), false);
                    }
                    ui.allocate_ui_with_layout(
                        egui::vec2(value_width, 42.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(value_width);
                            ui.label(
                                RichText::new(if draft.is_some() {
                                    if edit_policy::ROLE_BAN_ONLY {
                                        "数值 / Ban 编辑"
                                    } else {
                                        "修改数值"
                                    }
                                } else {
                                    "数值"
                                })
                                .strong(),
                            );
                        },
                    );
                    ui.end_row();
                    for (field, value) in fields {
                        field_cell(
                            ui,
                            name_width,
                            RichText::new(field.label).color(theme::TEXT).size(16.0),
                            false,
                        )
                        .on_hover_text(field.key);
                        if !compact {
                            field_cell(
                                ui,
                                key_width,
                                RichText::new(field.key)
                                    .monospace()
                                    .size(13.0)
                                    .color(theme::MUTED),
                                true,
                            )
                            .on_hover_text(field.key);
                        }
                        if let Some(edit) = draft
                            .as_deref_mut()
                            .filter(|_| edit_policy::can_edit(field.key))
                        {
                            edit_field(ui, field, version, *value, edit);
                        } else {
                            readonly_field(ui, field, *value, value_width);
                        }
                        ui.end_row();
                    }
                });
        });
}

fn readonly_field(ui: &mut egui::Ui, field: &crate::model::Field, value: u64, width: f32) {
    let display = if field.kind == FieldKind::Flag {
        if value == 0 {
            "否 · 0".into()
        } else {
            "是 · 1".into()
        }
    } else {
        value.to_string()
    };
    ui.allocate_ui_with_layout(
        egui::vec2(width, 42.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.label(
                RichText::new(display)
                    .color(theme::TEXT)
                    .size(18.0)
                    .strong(),
            )
            .on_hover_text(if edit_policy::can_edit(field.key) {
                "开启修改模式后可编辑"
            } else {
                "只读：此版本仅允许修改角色 Ban 和局内名字"
            });
        },
    );
}

fn edit_field(
    ui: &mut egui::Ui,
    field: &crate::model::Field,
    version: u32,
    original: u64,
    draft: &mut EditDraft,
) {
    let Some(input) = draft.values.get_mut(field.key) else {
        return;
    };
    ui.vertical(|ui| {
        ui.set_min_width(174.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        if field.kind == FieldKind::Flag {
            let mut enabled = input == "1";
            let label = if enabled { "是 / 1" } else { "否 / 0" };
            if ui
                .checkbox(&mut enabled, label)
                .on_hover_text(if edit_policy::is_role_ban(field.key) {
                    "是 / 1：设置角色 Ban；否 / 0：取消此 Ban 标记。"
                } else {
                    "勾选设为 1，取消设为 0。"
                })
                .changed()
            {
                *input = u64::from(enabled).to_string();
            }
        } else {
            ui.add(
                egui::TextEdit::singleline(input)
                    .desired_width(154.0)
                    .font(egui::FontId::proportional(17.0))
                    .text_color(theme::TEXT)
                    .char_limit(32),
            );
            let hint = if parse_input(input, field.maximum(version)).ok() == Some(original) {
                format!("范围 0–{}", field.maximum(version))
            } else {
                format!("原值 {original} · 0–{}", field.maximum(version))
            };
            ui.label(theme::muted(hint).size(13.0));
        }
        if field.kind == FieldKind::Identity {
            ui.label(
                RichText::new("ID 不匹配会清空统计")
                    .color(theme::AMBER)
                    .size(13.0),
            );
        } else if matches!(field.key, "lv_alienSelectionHMMR" | "lv_alienSelectionAMMR") {
            ui.label(theme::muted("游戏保存：100–3000").size(13.0));
        }
        match parse_input(input, field.maximum(version)) {
            Ok(number) if number != original && field.kind == FieldKind::Flag => {
                ui.label(
                    RichText::new(format!("原值 {original}"))
                        .color(theme::AMBER)
                        .size(13.0),
                );
            }
            Err(error) => {
                ui.label(RichText::new(error).color(theme::DANGER).size(13.0));
            }
            _ => {}
        }
    });
}

#[derive(Default, Clone)]
struct EditDraft {
    values: BTreeMap<String, String>,
    change_name: bool,
    player_name: String,
    #[cfg(not(feature = "role-ban-only"))]
    alien_preset: Option<crate::alien_preset::Preset>,
}

impl EditDraft {
    fn from_report(report: &BankReport) -> Self {
        Self {
            change_name: false,
            player_name: report.player_name.clone().unwrap_or_default(),
            values: report
                .fields
                .iter()
                .filter(|(key, _)| edit_policy::can_edit(key))
                .map(|(key, value)| (key.clone(), value.to_string()))
                .collect(),
            #[cfg(not(feature = "role-ban-only"))]
            alien_preset: None,
        }
    }

    fn changed_count(&self, report: &BankReport) -> usize {
        let name_changes = usize::from(
            self.desired_name()
                .is_some_and(|name| report.player_name.as_deref() != Some(name)),
        );
        name_changes
            + self
                .values
                .iter()
                .filter(|(key, value)| {
                    parse_input(value, u64::MAX).ok() != report.fields.get(*key).copied()
                })
                .count()
    }

    fn desired_name(&self) -> Option<&str> {
        (self.change_name && self.name_error().is_none()).then_some(self.player_name.as_str())
    }

    fn name_error(&self) -> Option<String> {
        if !self.change_name {
            return None;
        }
        if let Err(error) = bank::validate_player_name(&self.player_name) {
            return Some(error);
        }
        let bytes = self.player_name.len();
        if !(3..=17).contains(&bytes) {
            return Some(format!("当前名字为 {bytes} 个 UTF-8 字节，需为 3–17 个。"));
        }
        None
    }

    fn parsed(&self, report: &BankReport) -> Result<BTreeMap<String, u64>, String> {
        let mut fields = report.fields.clone();
        for field in FIELDS
            .iter()
            .filter(|field| field.since <= report.bank_version && edit_policy::can_edit(field.key))
        {
            if let Some(value) = self.values.get(field.key) {
                let value = parse_input(value, field.maximum(report.bank_version))
                    .map_err(|error| format!("{}：{error}", field.label))?;
                fields.insert(field.key.to_owned(), value);
            }
        }
        Ok(fields)
    }
}

fn parse_input(input: &str, maximum: u64) -> Result<u64, String> {
    let text = input.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("数值必须为非负整数".into());
    }
    let number = text.parse::<u64>().map_err(|_| "整数过大".to_owned())?;
    if number > maximum {
        return Err(format!("不能超过 {maximum}"));
    }
    Ok(number)
}

#[derive(Clone, Copy)]
enum EditAction {
    Begin,
    Save,
    SaveAs,
    Discard,
    #[cfg(not(feature = "role-ban-only"))]
    AlienPreset,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirmation {
    Discard,
    Close,
}

fn edit_help(ui: &mut egui::Ui, report: &BankReport) {
    ui.set_max_width(420.0);
    ui.label(RichText::new("保存前自动备份原文件").strong());
    if edit_policy::ROLE_BAN_ONLY {
        ui.label(
            "此版本允许修改现有角色 Ban 位和局内名字，其他统计字段只读。另存为只能创建新文件。",
        );
    } else {
        ui.label("全部现有字段均可编辑。玩家 ID 不匹配会清空统计；MMR 下次游戏保存时会被限制为 100–3000。");
        #[cfg(not(feature = "role-ban-only"))]
        ui.label(crate::alien_preset::LIMITS);
    }
    player_name_help(ui);
    ui.label("角色 Ban：是 / 1 表示排除，否 / 0 取消此标记，不保证获得该角色。");
    ui.label(
        RichText::new(
            "当前地图仅 1–10 对普通多人开局分配有效；列兵（11）无排除逻辑，12 为预留位。",
        )
        .color(theme::AMBER),
    );
    ui.label("单人、管理员指定和局内晋升不受此组 Ban 位限制。Bank v0–v2 无这些位。");
    ui.label("仅保存当前文件，不同步 XX.SC2Bank。切换存档前需保存或放弃修改。");
    if report.bank_version < 6 && !edit_policy::ROLE_BAN_ONLY {
        ui.label(RichText::new("旧版统计超过 9999 虽可保存，但此版地图下次存档可能写坏；建议统计与角色次数保持 ≤9999。").color(theme::AMBER));
    }
}

fn edit_toolbar(
    ui: &mut egui::Ui,
    report: &BankReport,
    draft: Option<&EditDraft>,
) -> Option<EditAction> {
    let mut action = None;
    if let Some(draft) = draft {
        let changes = draft.changed_count(report);
        let valid = draft.parsed(report).is_ok();
        ui.label(
            RichText::new(format!("修改中 · {changes} 项待保存"))
                .color(theme::ACCENT)
                .size(13.0),
        );
        if ui
            .add_enabled(valid && changes > 0, theme::primary_button("保存原存档"))
            .clicked()
        {
            action = Some(EditAction::Save);
        }
        if ui
            .add_enabled(valid, egui::Button::new("另存为…"))
            .clicked()
        {
            action = Some(EditAction::SaveAs);
        }
        if ui.button("退出修改").clicked() {
            action = Some(EditAction::Discard);
        }
    } else {
        let editable = !report.source_text.is_empty();
        let label = if edit_policy::ROLE_BAN_ONLY {
            "修改角色 Ban 与名字"
        } else {
            "开启修改模式"
        };
        if ui
            .add_enabled(editable, theme::primary_button(label))
            .on_disabled_hover_text("仅本地 SC2Bank 存档支持修改；录像快照为只读。")
            .clicked()
        {
            action = Some(EditAction::Begin);
        }
    }
    #[cfg(not(feature = "role-ban-only"))]
    {
        let available = crate::alien_preset::availability(report);
        let button = ui
            .add_enabled(available.is_ok(), egui::Button::new(crate::alien_preset::LABEL))
            .on_hover_text("设置 HMMR 9999 / AMMR 1650，检查并修正游戏次数、异形主宰胜场、职位次数和禁选/扣分条件；应用后可查看改动，再保存存档。");
        let button = if let Err(reason) = available {
            button.on_disabled_hover_text(reason)
        } else {
            button
        };
        if button.clicked() {
            action = Some(EditAction::AlienPreset);
        }
    }
    action
}

fn edit_notice(ui: &mut egui::Ui, report: &BankReport, draft: &EditDraft) {
    if report.fields.is_empty() {
        ui.label(theme::muted("此存档没有统计载荷，可仅修改局内名字。"));
    }
    if let Err(error) = draft.parsed(report) {
        ui.label(RichText::new(format!("无法保存：{error}")).color(theme::DANGER));
    }
    #[cfg(not(feature = "role-ban-only"))]
    if let Some(preset) = &draft.alien_preset {
        ui.label(RichText::new("异形选人预设已应用到草稿 · 保存后生效").color(theme::ACCENT));
        ui.label("HMMR 9999 · AMMR 1650 · 游戏次数至少 9 · 异形主宰胜场至少 5 · 职位 1–11 各至少 3 次");
        ui.label(theme::muted(crate::alien_preset::LIMITS).size(12.0));
        if draft
            .parsed(report)
            .ok()
            .and_then(|fields| crate::alien_preset::apply(report, &fields).ok())
            .is_some_and(|current| !current.changes.is_empty())
        {
            ui.colored_label(
                theme::AMBER,
                "预设条件已被后续编辑改变；可再次点击预设重新检查。",
            );
        }
        egui::CollapsingHeader::new(format!(
            "应用时修正了 {} 项（原值 → 预设值）",
            preset.changes.len()
        ))
        .show(ui, |ui| {
            if preset.changes.is_empty() {
                ui.label("全部可编辑条件已经符合预设，没有额外修改。");
            }
            for change in &preset.changes {
                ui.label(format!(
                    "{}：{} → {}；{}。",
                    change.label, change.before, change.after, change.reason
                ))
                .on_hover_text(&change.key);
            }
        });
    }
}

impl Viewer {
    fn has_unsaved_changes(&self) -> bool {
        self.edit
            .as_ref()
            .zip(self.report.and_then(|index| self.data.reports.get(index)))
            .is_some_and(|(draft, report)| draft.changed_count(report) > 0)
    }

    fn edit_action(&mut self, action: EditAction) {
        match action {
            EditAction::Begin => {
                if let Some(report) = self.report.and_then(|index| self.data.reports.get(index)) {
                    let draft = EditDraft::from_report(report);
                    if report.source_text.is_empty() {
                        self.error = Some("仅本地 SC2Bank 存档支持修改。".into());
                        return;
                    }
                    self.edit = Some(draft);
                    if edit_policy::ROLE_BAN_ONLY {
                        self.tab = Tab::RoleBans;
                    } else if self.tab != Tab::RoleBans {
                        self.tab = Tab::Fields;
                    }
                    self.filter.clear();
                    self.group = None;
                    self.error = None;
                    self.status = if edit_policy::ROLE_BAN_ONLY {
                        "角色 Ban 与名字修改模式：其他统计字段只读。"
                    } else {
                        "修改模式已开启，尚未写入任何文件。"
                    }
                    .into();
                }
            }
            EditAction::Discard => {
                if self.has_unsaved_changes() {
                    self.confirmation = Some(Confirmation::Discard);
                } else {
                    self.edit = None;
                }
            }
            EditAction::Save => self.save_edit(false),
            EditAction::SaveAs => self.save_edit(true),
            #[cfg(not(feature = "role-ban-only"))]
            EditAction::AlienPreset => self.apply_alien_preset(),
        }
    }

    #[cfg(not(feature = "role-ban-only"))]
    fn apply_alien_preset(&mut self) {
        let Some(report) = self.report.and_then(|index| self.data.reports.get(index)) else {
            return;
        };
        let mut draft = self
            .edit
            .clone()
            .unwrap_or_else(|| EditDraft::from_report(report));
        let result = draft
            .parsed(report)
            .and_then(|fields| crate::alien_preset::apply(report, &fields));
        match result {
            Ok(preset) => {
                for (key, value) in &preset.fields {
                    draft.values.insert(key.clone(), value.to_string());
                }
                self.status = format!(
                    "异形选人预设已检查，修正 {} 项；点击“保存原存档”写入并自动备份。",
                    preset.changes.len()
                );
                draft.alien_preset = Some(preset);
                self.edit = Some(draft);
                self.tab = Tab::Fields;
                self.filter.clear();
                self.group = None;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("无法应用异形选人预设：{error}")),
        }
    }

    fn save_edit(&mut self, save_as: bool) {
        let Some(report) = self.report.and_then(|index| self.data.reports.get(index)) else {
            return;
        };
        let Some(draft) = self.edit.as_ref() else {
            return;
        };
        if !save_as && draft.changed_count(report) == 0 {
            self.status = if draft.name_error().is_some() {
                "名字输入不符合要求，已跳过更名；没有其他修改，存档未写入。"
            } else {
                "没有需要保存的修改，存档未写入。"
            }
            .into();
            return;
        }
        let fields = match draft.parsed(report) {
            Ok(fields) => fields,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let destination = if save_as {
            let name = report
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("RR.SC2Bank");
            let mut dialog = rfd::FileDialog::new()
                .add_filter("存档", &["SC2Bank"])
                .set_file_name(name);
            if let Some(parent) = report.path.parent() {
                dialog = dialog.set_directory(parent);
            }
            let Some(path) = dialog.save_file() else {
                return;
            };
            path
        } else {
            report.path.clone()
        };
        let name_skipped = draft.name_error().is_some();
        match bank::save_modified(report, &fields, draft.desired_name(), &destination) {
            Ok(outcome) => {
                let backup = outcome
                    .backup_path
                    .as_ref()
                    .map_or_else(String::new, |path| format!("；备份：{}", path.display()));
                let name_notice = if name_skipped {
                    "；名字输入不符合要求，已跳过更名并保留存档原名"
                } else {
                    ""
                };
                self.status = format!(
                    "已保存：{}{name_notice}{backup}",
                    outcome.report.path.display()
                );
                self.error = None;
                self.edit = None;
                self.confirmation = None;
                if let Some((index, report)) = self
                    .data
                    .reports
                    .iter_mut()
                    .enumerate()
                    .find(|(_, report)| same_path(&report.path, &outcome.report.path))
                {
                    *report = outcome.report;
                    self.report = Some(index);
                } else {
                    if let Some(account) = self
                        .account
                        .and_then(|index| self.data.accounts.get_mut(index))
                    {
                        account.banks.push(scan::BankFile {
                            path: outcome.report.path.clone(),
                            server_id: outcome.report.server_id,
                        });
                    }
                    self.report = Some(self.data.reports.len());
                    self.data.reports.push(outcome.report);
                }
                self.account = self
                    .report
                    .and_then(|index| self.data.reports.get(index))
                    .and_then(|report| {
                        self.data.accounts.iter().position(|account| {
                            account
                                .banks
                                .iter()
                                .any(|file| same_path(&file.path, &report.path))
                        })
                    });
            }
            Err(error) => self.error = Some(format!("保存失败，修改仍保留：{error}")),
        }
    }

    fn edit_dialogs(&mut self, context: &egui::Context) {
        if self.confirmation == Some(Confirmation::Discard) {
            egui::Window::new("确认放弃修改")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(context, |ui| {
                    ui.label("当前修改将被丢弃，源文件不会改变。");
                    ui.horizontal(|ui| {
                        if ui.button("继续编辑").clicked() {
                            self.confirmation = None;
                        }
                        if ui.button("放弃修改").clicked() {
                            self.edit = None;
                            self.confirmation = None;
                            self.error = None;
                            self.status = "已放弃修改，源文件未改变。".into();
                        }
                    });
                });
        }
        if self.confirmation == Some(Confirmation::Close) {
            egui::Window::new("存档修改尚未保存")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(context, |ui| {
                    ui.label("关闭窗口将丢弃未保存的修改；保留修改需返回编辑并保存。");
                    ui.horizontal(|ui| {
                        if ui.button("返回编辑").clicked() {
                            self.confirmation = None;
                        }
                        if ui.button("放弃修改并关闭").clicked() {
                            self.allow_close = true;
                            self.confirmation = None;
                            context.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        }
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}
