//! Read-only explanation of observed selection facts and bounded score analysis.
use std::collections::BTreeMap;

use eframe::egui::{self, RichText};

use crate::alien_selection::ranges::{self, ComparisonAnalysis, PairwiseRelation, Verdict};
use crate::alien_selection::{
    mmr_choice_explanation, Availability, MmrModifier, MmrSource, PlayerScore, RoundAnalysis,
    ScoreRange, SelectionAnalysis,
};
use crate::replay::AlienSelectionReview;
use crate::theme;

use super::warnings;

#[derive(Clone, Copy, Default)]
struct ReviewState {
    round: u8,
    player: Option<u32>,
}

pub(super) fn view(ui: &mut egui::Ui, review: &AlienSelectionReview) {
    ui.label(RichText::new("异形选人复盘").size(23.0).strong());
    observed_results(ui, review);
    let Some(analysis) = review.analysis.as_ref() else {
        ui.add_space(12.0);
        ui.colored_label(theme::AMBER, "当前录像无法计算选人评分。");
        warnings(ui, &review.notes);
        return;
    };
    analysis_summary(ui, analysis);
    let state_id = ui.make_persistent_id((
        "selection_navigation",
        &review.player_names,
        observed_id(review, 1),
        observed_id(review, 2),
    ));
    let mut state = ui
        .ctx()
        .data_mut(|data| data.get_temp::<ReviewState>(state_id))
        .unwrap_or_default();
    round_navigation(ui, analysis, &mut state);
    if let Some(round) = analysis
        .rounds
        .iter()
        .find(|round| round.round == state.round)
    {
        resolve_player(&mut state, round, observed_id(review, round.round));
        ui.push_id(round.round, |ui| {
            round_summary(ui, round);
            player_workspace(ui, round, review, &mut state.player);
            round_notes(ui, round);
        });
    }
    ui.ctx().data_mut(|data| data.insert_temp(state_id, state));
    review_notes(ui, review, analysis);
}

fn player_label(id: u32, names: &BTreeMap<u32, String>) -> String {
    names.get(&id).map_or_else(
        || format!("玩家 {id}（姓名未映射）"),
        |name| format!("玩家 {id} · {name}"),
    )
}

fn round_role(round: u8) -> &'static str {
    if round == 1 {
        "最初的异形主宰"
    } else {
        "最初的异形继任者"
    }
}

fn observed_id(review: &AlienSelectionReview, round: u8) -> Option<u32> {
    review
        .observed
        .iter()
        .find(|observation| observation.role == round_role(round))
        .map(|observation| observation.player_id)
}

fn observed_results(ui: &mut egui::Ui, review: &AlienSelectionReview) {
    if ui.available_width() >= 440.0 {
        ui.columns(2, |columns| {
            for (column, round) in columns.iter_mut().zip([1, 2]) {
                observed_card(column, review, round);
            }
        });
    } else {
        for round in [1, 2] {
            observed_card(ui, review, round);
            ui.add_space(6.0);
        }
    }
}

fn observed_card(ui: &mut egui::Ui, review: &AlienSelectionReview, round: u8) {
    theme::panel().inner_margin(10.0).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 3.0;
        let observations: Vec<_> = review
            .observed
            .iter()
            .filter(|observation| observation.role == round_role(round))
            .collect();
        let title = if observations.is_empty() {
            round_role(round).to_owned()
        } else {
            format!("{} · 已证实", round_role(round))
        };
        ui.label(theme::muted(title).size(12.0));
        if observations.is_empty() {
            let single_round = review.analysis.as_ref().is_some_and(|analysis| {
                analysis.rounds.len() == 1 && !analysis.rounds.iter().any(|item| item.round == 2)
            });
            if round == 2 && single_round {
                ui.label(theme::muted("本局仅执行首轮 · 无第二轮").size(17.0));
            } else {
                ui.colored_label(theme::AMBER, "未确认 · 缺少当选证据");
            }
        }
        for observation in observations {
            let name = player_label(observation.player_id, &review.player_names);
            let short_name = review.player_names.get(&observation.player_id).map_or_else(
                || name.clone(),
                |name| format!("{} · {name}", observation.player_id),
            );
            ui.add(egui::Label::new(RichText::new(short_name).size(18.0).strong()).truncate())
                .on_hover_text(format!(
                    "{name}\n第 {round} 轮录像已证实，证据时点：游戏循环 {}",
                    observation.gameloop
                ));
        }
    });
}

fn analysis_summary(ui: &mut egui::Ui, analysis: &SelectionAnalysis) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(format!(
            "全房 HMMR 均值 {}",
            number(analysis.average_hmmr)
        ));
        ui.label(theme::muted("整数附加项 0–50 · 实际值未记录").size(12.0));
        ui.label(theme::muted("规则区间 · 含数值边界").size(12.0))
            .on_hover_text("未核实的客户端定点边界按保守区间传播；区间比较给出胜负条件，不代表实际分数或当选概率。");
    });
    if analysis.average_hmmr.is_none() {
        ui.colored_label(
            theme::AMBER,
            "全房均值未确定，无法计算完整总分；已知评分分项单独显示。",
        );
        egui::CollapsingHeader::new("全房均值缺失的输入与依据")
            .default_open(true)
            .show(ui, |ui| warnings(ui, &analysis.reasons));
    }
}

fn round_navigation(ui: &mut egui::Ui, analysis: &SelectionAnalysis, state: &mut ReviewState) {
    if !analysis
        .rounds
        .iter()
        .any(|round| round.round == state.round)
    {
        state.round = analysis.rounds.first().map_or(1, |round| round.round);
        state.player = None;
    }
    ui.scope(|ui| {
        theme::compact_controls(ui);
        ui.horizontal_wrapped(|ui| {
            for round in &analysis.rounds {
                let selected = state.round == round.round;
                let title = format!("第 {} 轮 · {}", round.round, round_role(round.round));
                let button = egui::Button::new(RichText::new(title).color(if selected {
                    theme::ON_ACCENT
                } else {
                    theme::TEXT
                }))
                .fill(if selected {
                    theme::ACCENT
                } else {
                    theme::PANEL
                })
                .min_size(egui::vec2(160.0, 28.0));
                if ui.add(button).clicked() && !selected {
                    state.round = round.round;
                    state.player = None;
                }
            }
        });
    });
}

fn resolve_player(state: &mut ReviewState, round: &RoundAnalysis, winner: Option<u32>) {
    if !round
        .players
        .iter()
        .any(|player| Some(player.id) == state.player)
    {
        state.player = winner
            .filter(|id| round.players.iter().any(|player| player.id == *id))
            .or_else(|| round.players.first().map(|player| player.id));
    }
}

fn round_summary(ui: &mut egui::Ui, round: &RoundAnalysis) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            if round.availability == Availability::CompleteRanges {
                theme::ACCENT
            } else {
                theme::AMBER
            },
            RichText::new(availability(round.availability)).size(12.0),
        );
        ui.label(format!("目标 MMR {}", number(round.target_mmr)));
        ui.label(match round.modifier {
            Some(MmrModifier::Full) => "全房均值 × 1",
            Some(MmrModifier::Half) => "全房均值 × 0.5",
            Some(MmrModifier::FullOrHalf) => "倍率处于边界：× 0.5 或 × 1",
            None => "目标倍率未确定",
        });
        ui.label(format!(
            "空候选回退：{}",
            match round.fallback {
                Some(true) => "已恢复全员",
                Some(false) => "未触发",
                None => "未确定",
            }
        ));
    });
}

fn round_notes(ui: &mut egui::Ui, round: &RoundAnalysis) {
    ui.add_space(8.0);
    egui::CollapsingHeader::new("本轮筛选、回退与评分说明").show(ui, |ui| {
        warnings(ui, &round.reasons);
        if round.fixed_order.is_some() {
            ui.label(theme::muted(
                "固定项排序只是比较的一部分；是否能反超还取决于附加项差值及同分顺序。",
            ));
        }
    });
}

fn player_workspace(
    ui: &mut egui::Ui,
    round: &RoundAnalysis,
    review: &AlienSelectionReview,
    selected: &mut Option<u32>,
) {
    let width = ui.available_width();
    if width >= 850.0 {
        let list_width = (width * 0.36).clamp(280.0, 370.0);
        let list_height = ui.available_height().clamp(260.0, 570.0);
        let detail_width = width - list_width - ui.spacing().item_spacing.x;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(list_width, list_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| player_list(ui, round, review, selected, list_height - 56.0),
            );
            ui.allocate_ui_with_layout(
                egui::vec2(detail_width, list_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("selection_player_details")
                        .max_height(list_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| selected_player(ui, round, review, *selected));
                },
            );
        });
    } else {
        player_picker(ui, round, review, selected);
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .id_salt("selection_player_details_narrow")
            .max_height(ui.available_height().max(300.0))
            .show(ui, |ui| selected_player(ui, round, review, *selected));
    }
}

fn player_picker(
    ui: &mut egui::Ui,
    round: &RoundAnalysis,
    review: &AlienSelectionReview,
    selected: &mut Option<u32>,
) {
    ui.label(theme::muted(format!("选择玩家 · 共 {} 人", round.players.len())).size(12.0));
    let label = selected.map_or_else(
        || "暂无玩家".into(),
        |id| player_label(id, &review.player_names),
    );
    egui::ComboBox::from_id_salt("selection_player")
        .selected_text(label)
        .width(ui.available_width())
        .truncate()
        .height(320.0)
        .show_ui(ui, |ui| {
            for player in &round.players {
                let label = format!(
                    "{} · {} · {}",
                    player_label(player.id, &review.player_names),
                    row_status(player),
                    verdict_label(
                        &ranges::compare(round, player.id, observed_id(review, round.round),)
                            .verdict
                    ),
                );
                if ui
                    .selectable_label(*selected == Some(player.id), label)
                    .on_hover_text(player_tooltip(player))
                    .clicked()
                {
                    *selected = Some(player.id);
                }
            }
        });
}

fn player_list(
    ui: &mut egui::Ui,
    round: &RoundAnalysis,
    review: &AlienSelectionReview,
    selected: &mut Option<u32>,
    height: f32,
) {
    theme::panel().inner_margin(10.0).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.strong(format!("玩家名单 · {} 人", round.players.len()))
            .on_hover_text("点击显示玩家评分，悬停显示排除原因与 MMR 依据。");
        egui::ScrollArea::vertical()
            .id_salt("selection_player_list")
            .max_height(height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for player in &round.players {
                    ui.push_id(player.id, |ui| {
                        if player_row(ui, player, review, round, *selected == Some(player.id)) {
                            *selected = Some(player.id);
                        }
                    });
                }
            });
    });
}

fn player_row(
    ui: &mut egui::Ui,
    player: &PlayerScore,
    review: &AlienSelectionReview,
    round: &RoundAnalysis,
    selected: bool,
) -> bool {
    let response = egui::Frame::NONE
        .fill(if selected {
            theme::ACCENT_SOFT
        } else {
            theme::CARD
        })
        .stroke(egui::Stroke::new(
            1.0,
            if selected {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ))
        .corner_radius(7.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            let name = player_label(player.id, &review.player_names);
            ui.add(egui::Label::new(RichText::new(&name).strong()).truncate())
                .on_hover_text(name);
            let status = if observed_id(review, round.round) == Some(player.id) {
                format!("录像当选 · {}", row_status(player))
            } else {
                row_status(player)
            };
            ui.add(
                egui::Label::new(
                    RichText::new(status)
                        .color(participation_color(player.eligible))
                        .size(12.0),
                )
                .truncate(),
            );
            ui.add(
                egui::Label::new(theme::muted(format!("MMR：{}", mmr_label(player))).size(12.0))
                    .truncate(),
            );
            let comparison = ranges::compare(round, player.id, observed_id(review, round.round));
            ui.add(
                egui::Label::new(
                    RichText::new(verdict_label(&comparison.verdict))
                        .size(12.0)
                        .color(verdict_color(&comparison.verdict)),
                )
                .truncate(),
            );
        })
        .response
        .interact(egui::Sense::click());
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!(
            "{}\n总分规则区间：{}",
            player_tooltip(player),
            score_range(player)
        ))
        .clicked()
}

fn row_status(player: &PlayerScore) -> String {
    if player.eligible != Some(false) || player.exclusions.is_empty() {
        return participation(player.eligible).into();
    }
    let labels: Vec<_> = player
        .exclusions
        .iter()
        .map(|item| item.reason.label())
        .collect();
    format!("不参与 · {}", labels.join(" / "))
}

fn player_tooltip(player: &PlayerScore) -> String {
    let mut lines = vec![mmr_choice_explanation(player)];
    lines.extend(
        player
            .exclusions
            .iter()
            .map(|exclusion| format!("{}：{}", exclusion.reason.label(), exclusion.detail)),
    );
    lines.join("\n")
}

fn selected_player(
    ui: &mut egui::Ui,
    round: &RoundAnalysis,
    review: &AlienSelectionReview,
    selected: Option<u32>,
) {
    theme::panel().inner_margin(12.0).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let Some(player) = round
            .players
            .iter()
            .find(|player| Some(player.id) == selected)
        else {
            ui.label("本轮没有可展示的玩家记录。");
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(player_label(player.id, &review.player_names)).size(20.0).strong());
            ui.colored_label(
                participation_color(player.eligible),
                RichText::new(participation(player.eligible)).size(12.0),
            );
            if observed_id(review, round.round) == Some(player.id) {
                ui.colored_label(theme::ACCENT, RichText::new("录像当选").size(12.0));
            }
        });
        exclusion_details(ui, player);
        comparison_details(ui, round, review, player);
        ui.add_space(4.0);
        ui.strong(format!("评分 MMR · {}", mmr_label(player)));
        ui.label(mmr_choice_explanation(player));
        ui.label(
            theme::muted("此地图未保存最初的异形继任者历史胜场；若显示 0，来源是普通开局初始化模型，并非录像历史统计。")
                .size(12.0),
        );
        ui.add_space(12.0);
        ui.strong("评分分项");
        ui.label(theme::muted("报名仅在成为本轮候选后生效；整数附加项的实际值未记录。").size(12.0));
        player_breakdown(ui, player);
        warnings(ui, &player.reasons);
    });
}

fn verdict_label(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::GuaranteedWin => "区间内保证当选",
        Verdict::ImpossibleWin => "区间内必定落选",
        Verdict::Conditional => "取决于具体差值",
        Verdict::Incomplete => "信息不足，保留条件",
        Verdict::Excluded => "候选规则排除",
        Verdict::Conflict => "模型与录像事实冲突",
    }
}

fn verdict_color(verdict: &Verdict) -> egui::Color32 {
    if matches!(verdict, Verdict::GuaranteedWin) {
        theme::ACCENT
    } else {
        theme::AMBER
    }
}

fn comparison_details(
    ui: &mut egui::Ui,
    round: &RoundAnalysis,
    review: &AlienSelectionReview,
    player: &PlayerScore,
) {
    let comparison = ranges::compare(round, player.id, observed_id(review, round.round));
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.strong("当选 / 落选条件");
        ui.colored_label(
            verdict_color(&comparison.verdict),
            RichText::new(verdict_label(&comparison.verdict)).strong(),
        );
    });
    ui.label(
        theme::muted("以下结论以本轮候选、输入区间与声明的遍历顺序为依据；录像当选事实单独列示。")
            .size(12.0),
    );
    if player.eligible != Some(false) {
        ui.label("总分 S = 固定项 B + 整数附加项 r，r ∈ [0, 50]。");
        own_roll_ranges(ui, &comparison);
        ui.label(
            theme::muted("必要区间只排除不可能取值；处于其中仍须同时满足对所有对手的比较条件。")
                .size(12.0),
        );
    }
    warnings(ui, &comparison.reasons);
    comparison_input_gaps(ui, round, review, player);
    observed_constraints(ui, &comparison, review, player.id);
    if player.eligible != Some(false) && !comparison.pairwise.is_empty() {
        ui.add_space(8.0);
        ui.strong("逐名比较");
        ui.label(theme::muted("两人的附加项差值 r本人 − r对手 只能落在 [−50, 50]。").size(12.0));
        for opponent in &comparison.pairwise {
            let relation = match opponent.relation {
                PairwiseRelation::Guaranteed => "必定压过",
                PairwiseRelation::Impossible => "无法压过",
                PairwiseRelation::Conditional => "需满足差值",
                PairwiseRelation::Unknown => "条件未齐",
            };
            egui::CollapsingHeader::new(format!(
                "{} · {relation}",
                player_label(opponent.opponent_id, &review.player_names),
            ))
            .id_salt(("selection_opponent", player.id, opponent.opponent_id))
            .default_open(
                matches!(opponent.relation, PairwiseRelation::Impossible)
                    || observed_id(review, round.round) == Some(opponent.opponent_id),
            )
            .show(ui, |ui| {
                if let Some(other) = round
                    .players
                    .iter()
                    .find(|item| item.id == opponent.opponent_id)
                {
                    ui.label(format!(
                        "对手固定项 B：{}；总分：{}。",
                        number(other.breakdown.fixed_range),
                        score_range(other),
                    ));
                }
                if opponent.opponent_eligible != Some(true) {
                    ui.colored_label(theme::AMBER, "对手候选身份尚未确认，此项只作条件比较。");
                }
                if let Some(required) = opponent.required_difference {
                    difference_condition(ui, required, "r本人 − r对手");
                } else {
                    let mut missing = Vec::new();
                    if player.breakdown.fixed_range.is_none() {
                        missing.push("本人固定项 B".to_owned());
                    }
                    if round
                        .players
                        .iter()
                        .find(|item| item.id == opponent.opponent_id)
                        .is_none_or(|item| item.breakdown.fixed_range.is_none())
                    {
                        missing.push(format!(
                            "{} 的固定项 B",
                            player_label(opponent.opponent_id, &review.player_names),
                        ));
                    }
                    if missing.is_empty() {
                        ui.label("双方固定项无法形成有效差值区间，不能计算此项门槛。");
                    } else {
                        ui.label(format!(
                            "未确定：{}；此项差值门槛暂不能计算。",
                            missing.join("、")
                        ));
                    }
                }
                ui.label(if player.id > opponent.opponent_id {
                    "同分修正 +1：本模型中对手先遍历，本人必须比对手总分至少高 1 分。"
                } else {
                    "同分修正 +0：本模型中本人先遍历，总分相同即可保留本人。"
                });
            });
        }
        ui.label(
            theme::muted(
                "地图使用严格大于才更换赢家，同分保留先遍历者；本模型按玩家编号升序处理遍历。",
            )
            .size(12.0),
        );
    }
}

fn comparison_input_gaps(
    ui: &mut egui::Ui,
    round: &RoundAnalysis,
    review: &AlienSelectionReview,
    player: &PlayerScore,
) {
    if player.eligible == Some(false) {
        return;
    }
    let mut gaps = Vec::new();
    if round.target_mmr.is_none() {
        if review
            .analysis
            .as_ref()
            .is_some_and(|analysis| analysis.average_hmmr.is_none())
        {
            gaps.push("全房 HMMR 均值未确定，无法得到本轮目标 MMR；受影响的是所有玩家的 MMR 距离扣分和总分，具体输入来源见上方展开的依据。".to_owned());
        } else if round.modifier.is_none() {
            gaps.push("本轮目标倍率未确定，无法由全房 HMMR 均值得到目标 MMR。".to_owned());
        } else {
            gaps.push("本轮目标 MMR 未形成有效区间，暂不能计算距离扣分。".to_owned());
        }
    }
    if player.used_mmr.is_none() {
        let source = match player.mmr_source {
            Some(MmrSource::Human) => "HMMR（人类 MMR）",
            Some(MmrSource::Alien) => "AMMR（异形 MMR）",
            None => "评分 MMR（胜场分支与数值尚未齐全）",
        };
        gaps.push(format!(
            "本人 {source} 未形成有效区间，无法计算本人的距离扣分。"
        ));
    } else if player.breakdown.fixed_range.is_none() && round.target_mmr.is_some() {
        gaps.push("本人固定项合计 B 未形成有效区间，暂不能与对手计算差值。".to_owned());
    }
    if player.eligible.is_none() {
        gaps.push("本人的候选身份未确认，尚不能确定是否进入本轮计分。".to_owned());
    }
    if round.fallback.is_none() {
        gaps.push("候选是否为空尚未确认，无法确定是否需要恢复全员参与。".to_owned());
    }
    let opponents: Vec<_> = round
        .players
        .iter()
        .filter(|opponent| opponent.id != player.id && opponent.eligible != Some(false))
        .collect();
    let uncertain_candidates: Vec<_> = opponents
        .iter()
        .filter(|opponent| opponent.eligible.is_none())
        .map(|opponent| player_label(opponent.id, &review.player_names))
        .collect();
    if !uncertain_candidates.is_empty() {
        gaps.push(format!(
            "对手候选身份未确认：{}。",
            uncertain_candidates.join("、")
        ));
    }
    if round.target_mmr.is_some() {
        let missing_scores: Vec<_> = opponents
            .iter()
            .filter(|opponent| opponent.breakdown.fixed_range.is_none())
            .map(|opponent| player_label(opponent.id, &review.player_names))
            .collect();
        if !missing_scores.is_empty() {
            gaps.push(format!(
                "对手固定项 B 未确定：{}；与这些玩家的差值暂不能计算。",
                missing_scores.join("、")
            ));
        }
    }
    if !gaps.is_empty() {
        ui.add_space(6.0);
        ui.colored_label(theme::AMBER, RichText::new("当前缺少的比较输入").strong());
        warnings(ui, &gaps);
        ui.label(
            theme::muted("已具备固定项区间的双方仍可逐名比较；非单点区间本身不会阻止计算。")
                .size(12.0),
        );
    }
}

fn own_roll_ranges(ui: &mut egui::Ui, comparison: &ComparisonAnalysis) {
    let possible = comparison.possible_roll.map_or_else(
        || {
            if matches!(comparison.verdict, Verdict::ImpossibleWin) {
                "空集：0–50 内没有满足胜出条件的取值".into()
            } else {
                "未确定".into()
            }
        },
        interval,
    );
    ui.label(format!("本人当选所需的附加项必要区间：{possible}。"));
    let guaranteed = comparison.guaranteed_roll.map_or_else(
        || {
            if matches!(
                comparison.verdict,
                Verdict::Conditional | Verdict::ImpossibleWin
            ) {
                "未得到可保证区间，仍须结合对手取值比较".into()
            } else {
                "未确定".into()
            }
        },
        interval,
    );
    ui.label(format!("本人保证胜出的附加项区间：{guaranteed}。"));
}

fn difference_condition(ui: &mut egui::Ui, required: ScoreRange, difference: &str) {
    if required.min == required.max {
        ui.strong(format!("胜出条件：{difference} ≥ {}。", required.min));
    } else {
        ui.label(format!(
            "胜出门槛 q ∈ {}；需满足 {difference} ≥ q。",
            interval(required)
        ));
        ui.label(format!(
            "差值至少 {}，可跨过全部固定项边界；差值低于 {}，则无法胜出；中间还取决于未确定分项。",
            required.max, required.min,
        ));
    }
}

fn observed_constraints(
    ui: &mut egui::Ui,
    comparison: &ComparisonAnalysis,
    review: &AlienSelectionReview,
    player_id: u32,
) {
    let Some(observed) = comparison.observed_constraint.as_ref() else {
        return;
    };
    ui.add_space(8.0);
    ui.strong("由录像当选事实反推");
    ui.label(format!(
        "录像已证实：{} 当选。",
        player_label(observed.winner_id, &review.player_names),
    ));
    if observed.conflict {
        ui.colored_label(theme::AMBER, "当前候选或分数区间不能解释这个结果；请检查缺失输入和开局状态，不能编造一个附加值来吻合录像。");
    } else {
        ui.label(format!(
            "当选者附加项必要区间：{}。",
            number(observed.winner_roll)
        ));
        if observed.winner_id != player_id {
            ui.label(format!(
                "在该当选事实下，本人附加项必须满足的范围：{}。",
                number(observed.selected_player_roll)
            ));
            if let Some(required) = observed.winner_required_difference {
                difference_condition(ui, required, "r当选者 − r本人");
            }
        }
        ui.label(
            theme::muted("以上是当选事实要求满足的数值约束，不是录像记录的实际附加值。").size(12.0),
        );
    }
    warnings(ui, &observed.reasons);
}

fn exclusion_details(ui: &mut egui::Ui, player: &PlayerScore) {
    match player.eligible {
        Some(false) => {
            ui.add_space(8.0);
            ui.colored_label(theme::AMBER, RichText::new("不参与本轮竞选的原因").strong());
            if player.exclusions.is_empty() {
                ui.label("具体排除原因未确定，不能由未报名或扣分反推。");
            }
            for exclusion in &player.exclusions {
                ui.label(RichText::new(exclusion.reason.label()).strong());
                ui.label(&exclusion.detail);
            }
            ui.label(
                theme::muted("本轮不计分指选人阶段，不代表局后结算。下方保留可核实的分项。")
                    .size(12.0),
            );
        }
        None => {
            ui.colored_label(theme::AMBER, "候选身份信息不全，无法确认是否参与本轮。");
        }
        Some(true) => {}
    }
}

fn availability(value: Availability) -> &'static str {
    match value {
        Availability::CompleteRanges => "规则区间可比较",
        Availability::Partial => "可核实部分因素",
        Availability::Insufficient => "信息不足",
    }
}

fn number(value: Option<ScoreRange>) -> String {
    value.map_or_else(|| "未确定".into(), interval)
}

fn interval(value: ScoreRange) -> String {
    if value.min == value.max {
        value.min.to_string()
    } else {
        format!("[{}, {}]", value.min, value.max)
    }
}

fn signed_range(value: Option<ScoreRange>) -> String {
    value.map_or_else(
        || "未确定".into(),
        |range| {
            if range.min == range.max {
                format!("{:+}", range.min)
            } else {
                format!("[{:+}, {:+}]", range.min, range.max)
            }
        },
    )
}

fn contribution(value: Option<i32>, amount: i32) -> String {
    value.map_or_else(
        || format!("[{:+}, {:+}]（状态未知）", amount.min(0), amount.max(0)),
        |value| format!("{value:+}"),
    )
}

fn mmr_label(player: &PlayerScore) -> String {
    let source = match player.mmr_source {
        Some(MmrSource::Human) => "HMMR（人类）",
        Some(MmrSource::Alien) => "AMMR（异形）",
        None => "未确定",
    };
    format!("{source} · {}", number(player.used_mmr))
}

fn participation(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "参与本轮",
        Some(false) => "本轮不计分",
        None => "是否参与未确定",
    }
}

fn participation_color(value: Option<bool>) -> egui::Color32 {
    if value == Some(true) {
        theme::ACCENT
    } else {
        theme::AMBER
    }
}

fn score_range(player: &PlayerScore) -> String {
    if player.eligible == Some(false) {
        return "不适用".into();
    }
    player
        .breakdown
        .total_range
        .map_or_else(|| "无法计算".into(), interval)
}

fn player_breakdown(ui: &mut egui::Ui, player: &PlayerScore) {
    let breakdown = &player.breakdown;
    egui::Grid::new("score_factors")
        .striped(true)
        .min_row_height(22.0)
        .spacing([18.0, 4.0])
        .show(ui, |ui| {
            ui.label("MMR 差距项");
            ui.label(signed_range(breakdown.mmr_penalty));
            ui.end_row();
            for (label, value, amount) in [
                ("报名项", breakdown.preference_bonus, 100),
                ("降优先级项", breakdown.deprioritized_penalty, -25),
                ("上局离开项", breakdown.left_penalty, -100),
                ("新角色项", breakdown.new_role_penalty, -250),
                ("异形主宰限制项", breakdown.host_ban_penalty, -5000),
                ("新玩家项", breakdown.new_player_penalty, -5000),
                ("额外条件项", breakdown.requirement_penalty, -200),
            ] {
                ui.label(label);
                ui.label(contribution(value, amount));
                ui.end_row();
            }
            ui.strong("固定项合计 B");
            ui.strong(number(breakdown.fixed_range));
            ui.end_row();
            ui.label("整数附加项 r");
            ui.label(if player.eligible == Some(false) {
                "不适用"
            } else {
                "[0, 50]（实际值未记录）"
            });
            ui.end_row();
            ui.strong("总分规则区间 B + r");
            ui.strong(score_range(player));
            ui.end_row();
        });
}

fn review_notes(ui: &mut egui::Ui, review: &AlienSelectionReview, analysis: &SelectionAnalysis) {
    ui.add_space(8.0);
    ui.collapsing("本局计算依据与不确定项", |ui| {
        ui.label("当选身份来自录像证据；当选和落选条件由评分区间及同分规则推导。");
        ui.label(
            "报名 +100 仅对本轮候选生效；缺失项保持“未确定”。本轮不计分指选人评分，并非局后结算。",
        );
        ui.label("客户端定点边界尚未实测，数值以保守区间传播。区间不是实际分数、概率，也不保证其中每种组合均能由游戏产生。");
        warnings(ui, &analysis.reasons);
    });
    ui.collapsing("MMR 规则与初始化来源", |ui| {
        ui.label("房间目标以全体参与者的 HMMR 均值为基础；个人距离评分另按胜场选择 MMR。");
        ui.label("异形主宰胜场 + 最初的异形继任者胜场达到 5：使用 AMMR；不足 5：使用 HMMR。");
        ui.label("MMR 与本轮目标的差距每满 10 点扣 1 分，距离越远扣分越多。");
        ui.label(theme::muted(
            "此地图版本未保存或读取最初的异形继任者胜场；复盘按普通开局初值 0 计算。初值来自初始化规则，不属于录像中的历史统计。",
        ));
    });
    egui::CollapsingHeader::new("录像复盘依据与限制")
        .default_open(analysis.rounds.is_empty())
        .show(ui, |ui| warnings(ui, &review.notes));
}
