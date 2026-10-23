use eframe::egui::{self, RichText};

use crate::model::{BankReport, FieldKind};
use crate::replay::{ReplayBank, ReplayPlayer, ReplayReport};
use crate::theme;

use super::Tab;

#[path = "alien_selection_ui.rs"]
mod alien_selection_ui;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Players,
    AlienSelection,
    Source,
}

#[derive(Clone, Default)]
struct PlayerFilter {
    query: String,
    missing_only: bool,
}

pub(super) fn navigation(
    ui: &mut egui::Ui,
    page: &mut Page,
    selected: &mut Option<usize>,
    active: bool,
) -> bool {
    let mut clicked = false;
    for (destination, title, caption) in [
        (Page::Players, "玩家总览", "玩家数据 · 存档快照"),
        (Page::AlienSelection, "异形选人复盘", "当选结果 · 评分依据"),
        (Page::Source, "录像资料", "文件信息 · 审查提示"),
    ] {
        if theme::nav_item(ui, active && *page == destination, title, caption).clicked() {
            clicked = true;
            *page = destination;
            if *page == Page::Players {
                *selected = None;
            }
        }
    }
    clicked
}

pub(super) fn view(
    ui: &mut egui::Ui,
    replay: &mut ReplayReport,
    page: &mut Page,
    selected: &mut Option<usize>,
    tab: &mut Tab,
    filter: &mut String,
    group: &mut Option<FieldKind>,
) {
    if selected.is_some_and(|index| index >= replay.players.len()) {
        *selected = None;
    }
    let scroll_id = match page {
        Page::Players => "replay_players_content",
        Page::AlienSelection => "alien_selection_content",
        Page::Source => "replay_source_content",
    };
    let detail_id = (*page == Page::Players).then_some(*selected).flatten();
    ui.push_id(replay.path.clone(), |ui| {
        egui::ScrollArea::vertical()
            .id_salt((scroll_id, detail_id))
            .auto_shrink([false, false])
            .show(ui, |ui| match page {
                Page::AlienSelection => alien_selection_ui::view(ui, &replay.alien_selection),
                Page::Source => replay_details(ui, replay),
                Page::Players => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("玩家存档")
                                .size(25.0)
                                .strong()
                                .line_height(Some(30.0)),
                        );
                        ui.label(theme::muted(&replay.title).size(13.0));
                    });
                    ui.add_space(6.0);
                    if selected.is_some() {
                        player_navigation(ui, &replay.players, selected);
                    }
                    if let Some(index) = *selected {
                        if let Some(player) = replay.players.get_mut(index) {
                            ui.push_id(index, |ui| player_view(ui, player, tab, filter, group));
                        }
                    } else {
                        player_table(ui, &replay.players, selected);
                    }
                }
            });
    });
}

fn bank_name(bank: Option<&ReplayBank>) -> &str {
    bank.and_then(ReplayBank::player_name)
        .filter(|name| !name.is_empty())
        .unwrap_or("未记录")
}

fn selected_bank(player: &ReplayPlayer) -> Option<&ReplayBank> {
    player
        .selected_bank
        .and_then(|index| player.banks.get(index))
}

fn selected_report(player: &ReplayPlayer) -> Option<&BankReport> {
    selected_bank(player).and_then(|bank| bank.report.as_ref())
}

fn display_value(player: &ReplayPlayer, key: &str) -> String {
    selected_report(player)
        .and_then(|report| report.metric(key))
        .map_or_else(|| "未记录".into(), |number| number.to_string())
}

fn slot(player: &ReplayPlayer) -> String {
    player.slot_id.map_or_else(
        || "—".into(),
        |id| u64::from(id).saturating_add(1).to_string(),
    )
}

fn player_matches(player: &ReplayPlayer, filter: &PlayerFilter) -> bool {
    if filter.missing_only && selected_report(player).is_some() {
        return false;
    }
    let query = filter.query.trim().to_lowercase();
    [
        player.name.as_str(),
        player.toon_handle.as_str(),
        bank_name(selected_bank(player)),
    ]
    .iter()
    .any(|text| text.to_lowercase().contains(&query))
        || slot(player) == query
}

fn player_table(ui: &mut egui::Ui, players: &[ReplayPlayer], selected: &mut Option<usize>) {
    ui.spacing_mut().item_spacing.y = 4.0;
    let id = ui.make_persistent_id("player_filter");
    let mut filter = ui.data_mut(|data| data.get_temp::<PlayerFilter>(id).unwrap_or_default());
    ui.horizontal_wrapped(|ui| {
        theme::compact_controls(ui);
        ui.add(
            egui::TextEdit::singleline(&mut filter.query)
                .id_salt("player_search")
                .hint_text("搜索玩家、局内名字或句柄")
                .desired_width((ui.available_width() - 150.0).clamp(160.0, 320.0)),
        );
        ui.checkbox(&mut filter.missing_only, "仅未记录数据");
        if !filter.query.is_empty() && ui.small_button("清除").clicked() {
            filter.query.clear();
        }
    });
    let visible: Vec<_> = players
        .iter()
        .enumerate()
        .filter(|(_, player)| player_matches(player, &filter))
        .collect();
    ui.label(
        theme::muted(format!(
            "显示 {} / {} 名玩家 · 点击任意一行查看详情 · 录像只读",
            visible.len(),
            players.len()
        ))
        .size(12.0)
        .line_height(Some(17.0)),
    );
    ui.data_mut(|data| data.insert_temp(id, filter));
    player_table_header(ui);
    for (position, (index, player)) in visible.iter().enumerate() {
        player_row(ui, player, *index, position % 2 == 0, selected);
    }
    if visible.is_empty() {
        ui.add_space(16.0);
        ui.label("无符合当前搜索及筛选条件的玩家。");
    }
    ui.add_space(10.0);
    ui.collapsing("数据来源与统计口径", |ui| {
        ui.label("显示录像中同步的存档快照；默认显示最近主存档快照；主存档缺少核心节时才选含核心节的备份，不代表本局结算结果。");
        ui.label("局内名字取自当前快照；未记录的数值不会补成 0。录像资料页保留来源和审查提示。");
    });
}

const TABLE_COLUMNS: [(&str, f32); 6] = [
    ("玩家 / 局内名字", 0.30),
    ("游戏次数", 0.10),
    ("游戏胜利", 0.10),
    ("HMMR", 0.13),
    ("AMMR", 0.13),
    ("快照 / 读取状态", 0.24),
];

fn table_cell(ui: &mut egui::Ui, width: f32, content: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            content(ui);
        },
    );
}

fn player_table_header(ui: &mut egui::Ui) {
    egui::Frame::NONE
        .inner_margin(egui::vec2(8.0, 5.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().interact_size.y = 18.0;
            let width = (ui.available_width() - 40.0).max(0.0);
            ui.horizontal_top(|ui| {
                for (title, fraction) in TABLE_COLUMNS {
                    table_cell(ui, width * fraction, |ui| {
                        ui.add(
                            egui::Label::new(
                                theme::muted(title).size(12.0).line_height(Some(16.0)),
                            )
                            .truncate(),
                        )
                        .on_hover_text(title);
                    });
                }
            });
        });
}

fn player_row(
    ui: &mut egui::Ui,
    player: &ReplayPlayer,
    index: usize,
    shaded: bool,
    selected: &mut Option<usize>,
) {
    let row = egui::Frame::NONE
        .fill(if shaded { theme::ROW_ALT } else { theme::PANEL })
        .corner_radius(5.0)
        .inner_margin(egui::vec2(8.0, 4.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 1.0);
            ui.spacing_mut().interact_size.y = 18.0;
            let width = (ui.available_width() - 40.0).max(0.0);
            ui.horizontal_top(|ui| {
                for (column, (_, fraction)) in TABLE_COLUMNS.into_iter().enumerate() {
                    table_cell(ui, width * fraction, |ui| player_cell(ui, player, column));
                }
            });
        });
    if ui
        .interact(
            row.response.rect,
            ui.scope_id().with(("player_row", index)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(player_tooltip(player))
        .clicked()
    {
        *selected = Some(index);
    }
}

fn player_tooltip(player: &ReplayPlayer) -> String {
    let snapshot = selected_bank(player).map_or_else(|| "无快照".into(), snapshot_label);
    let mut lines = vec![
        format!("槽位 {} · {}", slot(player), player.name),
        format!("局内名字：{}", bank_name(selected_bank(player))),
        format!("句柄：{}", player.toon_handle),
        format!("当前快照：{snapshot} · 共 {} 份", player.banks.len()),
    ];
    for (label, key) in [
        ("游戏次数", "lv_statsGamesPlayed"),
        ("游戏胜利", "lv_statsGamesWon"),
        ("HMMR", "lv_alienSelectionHMMR"),
        ("AMMR", "lv_alienSelectionAMMR"),
    ] {
        lines.push(format!("{label}：{}", display_value(player, key)));
    }
    lines.join("\n")
}

fn player_cell(ui: &mut egui::Ui, player: &ReplayPlayer, column: usize) {
    match column {
        0 => {
            let name = format!("{} · {}", slot(player), player.name);
            ui.add(
                egui::Label::new(
                    RichText::new(&name)
                        .size(14.0)
                        .line_height(Some(18.0))
                        .strong()
                        .color(theme::ACCENT),
                )
                .truncate(),
            )
            .on_hover_text(format!("{name}\n{}", player.toon_handle));
            let name = format!("局内名字：{}", bank_name(selected_bank(player)));
            ui.add(
                egui::Label::new(theme::muted(&name).size(12.0).line_height(Some(16.0))).truncate(),
            )
            .on_hover_text(name);
        }
        1..=4 => {
            let key = match column {
                1 => "lv_statsGamesPlayed",
                2 => "lv_statsGamesWon",
                3 => "lv_alienSelectionHMMR",
                _ => "lv_alienSelectionAMMR",
            };
            let value = display_value(player, key);
            ui.add(
                egui::Label::new(RichText::new(&value).size(14.0).line_height(Some(18.0)))
                    .truncate(),
            )
            .on_hover_text(value);
        }
        _ => player_status(ui, player),
    }
}

fn player_status(ui: &mut egui::Ui, player: &ReplayPlayer) {
    let snapshot = selected_bank(player).map_or_else(
        || format!("{} 份 · 未选择", player.banks.len()),
        |bank| format!("{} 份 · {}", player.banks.len(), bank.name),
    );
    ui.add(egui::Label::new(theme::muted(&snapshot).size(12.0).line_height(Some(16.0))).truncate())
        .on_hover_text(snapshot);
    let (status, color) = selected_report(player).map_or_else(
        || ("未记录统计载荷".into(), theme::AMBER),
        |report| {
            (
                format!("{} 字段 · v{}", report.fields.len(), report.bank_version),
                theme::TEXT,
            )
        },
    );
    ui.add(
        egui::Label::new(
            RichText::new(&status)
                .size(12.0)
                .line_height(Some(16.0))
                .color(color),
        )
        .truncate(),
    )
    .on_hover_text(status);
}

fn player_navigation(ui: &mut egui::Ui, players: &[ReplayPlayer], selected: &mut Option<usize>) {
    let current = selected.unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        theme::compact_controls(ui);
        if ui.button("← 玩家总览").clicked() {
            *selected = None;
        }
        let label = players.get(current).map_or_else(String::new, |player| {
            format!("{} · {}", slot(player), player.name)
        });
        let width = (ui.available_width() - 180.0).clamp(120.0, 280.0);
        table_cell(ui, width, |ui| {
            egui::ComboBox::from_id_salt("quick_player")
                .selected_text(label)
                .width(width)
                .truncate()
                .show_ui(ui, |ui| {
                    ui.set_width(320.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for (index, player) in players.iter().enumerate() {
                        ui.selectable_value(
                            selected,
                            Some(index),
                            format!("{} · {}", slot(player), player.name),
                        );
                    }
                });
        });
        if ui
            .add_enabled(current > 0, egui::Button::new("上一位"))
            .clicked()
        {
            *selected = current.checked_sub(1);
        }
        if ui
            .add_enabled(current + 1 < players.len(), egui::Button::new("下一位"))
            .clicked()
        {
            *selected = Some(current + 1);
        }
    });
    ui.add_space(10.0);
}

fn player_view(
    ui: &mut egui::Ui,
    player: &mut ReplayPlayer,
    tab: &mut Tab,
    filter: &mut String,
    group: &mut Option<FieldKind>,
) {
    theme::panel().inner_margin(14.0).show(ui, |ui| {
        ui.add(
            egui::Label::new(
                RichText::new(format!("槽位 {} · {}", slot(player), player.name))
                    .strong()
                    .size(19.0),
            )
            .wrap(),
        );
        ui.add(egui::Label::new(theme::muted(&player.toon_handle).size(12.0)).wrap());
        snapshot_picker(ui, player);
        if let Some(bank) = selected_bank(player) {
            ui.label(format!("此快照中的局内名字：{}", bank_name(Some(bank))));
            if let Some(report) = bank.report.as_ref() {
                theme::status_badge(
                    ui,
                    &format!(
                        "已读取 {} 字段 · Bank v{}",
                        report.fields.len(),
                        report.bank_version
                    ),
                    theme::ACCENT,
                );
            } else {
                theme::status_badge(ui, "未记录可解码的统计载荷", theme::AMBER);
            }
        }
    });
    warning_group(ui, "player_warnings", "玩家审查提示", &player.warnings);
    let Some(bank) = selected_bank(player) else {
        ui.label(RichText::new("录像没有记录此玩家的存档快照。").color(theme::AMBER));
        return;
    };
    warning_group(ui, "bank_warnings", "快照审查提示", &bank.warnings);
    if let Some(report) = bank.report.as_ref() {
        player_report(ui, report, tab, filter, group);
    } else {
        ui.label(theme::muted(
            "快照中无可解码的统计载荷，统计数值标记为未记录；原始记录列于下方。",
        ));
    }
    raw_bank(ui, bank);
}

fn player_report(
    ui: &mut egui::Ui,
    report: &BankReport,
    tab: &mut Tab,
    filter: &mut String,
    group: &mut Option<FieldKind>,
) {
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        theme::compact_controls(ui);
        ui.selectable_value(tab, Tab::Overview, "存档概览");
        ui.selectable_value(
            tab,
            Tab::Fields,
            format!("全部字段 · {}", report.fields.len()),
        );
        ui.selectable_value(tab, Tab::RoleBans, "角色 Ban");
    });
    ui.add_space(10.0);
    match tab {
        Tab::Overview => super::overview(ui, report),
        Tab::Fields => super::fields_view(ui, report, filter, group, None, false),
        Tab::RoleBans => super::fields_view(ui, report, filter, group, None, true),
    }
    ui.collapsing("载荷读取说明", |ui| {
        ui.label(theme::muted(&report.decode_note));
    });
}

fn snapshot_picker(ui: &mut egui::Ui, player: &mut ReplayPlayer) {
    ui.horizontal_wrapped(|ui| {
        theme::compact_controls(ui);
        ui.label("同步快照");
        let label = selected_bank(player).map_or_else(|| "无快照".into(), snapshot_label);
        egui::ComboBox::from_id_salt("replay_snapshot")
            .selected_text(label)
            .width(ui.available_width().min(360.0))
            .truncate()
            .show_ui(ui, |ui| {
                ui.set_width(360.0);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                for (index, bank) in player.banks.iter().enumerate() {
                    let status = if bank.report.is_some() {
                        "可解码"
                    } else {
                        "未记录/不可解码"
                    };
                    ui.selectable_value(
                        &mut player.selected_bank,
                        Some(index),
                        format!("#{} · {} · {status}", index + 1, snapshot_label(bank)),
                    );
                }
            });
    });
}

fn snapshot_label(bank: &ReplayBank) -> String {
    format!("{} · 游戏循环 {}", bank.name, bank.gameloop)
}

fn warnings(ui: &mut egui::Ui, messages: &[String]) {
    for message in messages {
        ui.add(egui::Label::new(RichText::new(message).color(theme::AMBER)).wrap());
    }
}

fn warning_group(ui: &mut egui::Ui, id: &str, title: &str, messages: &[String]) {
    if !messages.is_empty() {
        egui::CollapsingHeader::new(
            RichText::new(format!("{title} · {}", messages.len())).color(theme::AMBER),
        )
        .id_salt(id)
        .show(ui, |ui| warnings(ui, messages));
    }
}

fn raw_bank(ui: &mut egui::Ui, bank: &ReplayBank) {
    ui.add_space(10.0);
    ui.collapsing("原始存档键与同步信息", |ui| {
        ui.add(
            egui::Label::new(format!(
                "Bank {} · 起始循环 {} · 结束循环 {} · 句柄 {}",
                bank.name,
                bank.gameloop,
                bank.end_gameloop
                    .map_or_else(|| "未结束".into(), |value| value.to_string()),
                bank.toon_handle
            ))
            .wrap(),
        );
        ui.label(format!(
            "签名字节数：{}（未验证引擎签名）",
            bank.signature.len()
        ));
        for (section, keys) in &bank.sections {
            ui.collapsing(section, |ui| {
                for (name, key) in keys {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{name} · 类型 {}", key.value_type)).strong(),
                        )
                        .wrap(),
                    );
                    ui.add(egui::Label::new(&key.data).wrap());
                    for value in &key.values {
                        ui.add(
                            egui::Label::new(format!(
                                "{} · 类型 {}：{}",
                                value.name, value.value_type, value.data
                            ))
                            .wrap(),
                        );
                    }
                }
            });
        }
    });
}

fn replay_details(ui: &mut egui::Ui, replay: &ReplayReport) {
    theme::section_heading(ui, "录像资料", "文件来源、同步记录与审查提示");
    theme::panel().inner_margin(16.0).show(ui, |ui| {
        ui.strong(&replay.title);
        ui.add(egui::Label::new(theme::muted(replay.path.display().to_string())).wrap());
        ui.add_space(10.0);
        for (label, value) in [
            ("录像 build", replay.base_build.to_string()),
            ("解析协议", replay.protocol_build.to_string()),
            ("游戏循环", replay.elapsed_game_loops.to_string()),
            ("Bank 事件", replay.bank_event_count.to_string()),
            ("玩家人数", replay.players.len().to_string()),
        ] {
            ui.horizontal(|ui| {
                ui.add_sized([95.0, 20.0], egui::Label::new(theme::muted(label)));
                ui.strong(value);
            });
        }
    });
    ui.add_space(12.0);
    if replay.warnings.is_empty() {
        ui.label(theme::muted("当前没有额外的录像审查提示。"));
    } else {
        warning_group(ui, "replay_warnings", "录像审查提示", &replay.warnings);
    }
    ui.label(theme::muted("各玩家及快照的审查提示保留在对应玩家详情中。"));
    ui.add_space(12.0);
    ui.collapsing(
        format!("未匹配玩家的快照 · {}", replay.unmapped_banks.len()),
        |ui| {
            ui.label("以下快照无法可靠匹配玩家；保留原始键，供进一步审查。");
            for (index, bank) in replay.unmapped_banks.iter().enumerate() {
                ui.push_id(index, |ui| {
                    ui.strong(format!("#{} · {}", index + 1, snapshot_label(bank)));
                    warning_group(ui, "unmapped_warnings", "快照审查提示", &bank.warnings);
                    raw_bank(ui, bank);
                });
            }
        },
    );
}
