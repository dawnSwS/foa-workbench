//! Read-only reconstruction of the audited map's alien-selection formula.
//! Inputs are effective state at selection time, never unprocessed bank values.

/// Fixed predicates from gf_SelfingList9 / gf_ConsolePermissions2 in this map.
/// Bank edits cannot change these identity-based exclusions.
pub fn hardcoded_game_ban(handle: &str) -> bool {
    matches!(
        handle,
        "5-S2-1-12355973" | "5-S2-1-12377679" | "2-S2-1-11076112" | "0"
    )
}

pub fn hardcoded_host_ban(handle: &str) -> bool {
    matches!(
        handle,
        "1-S2--1-2203402" | "1-S2-2-800148" | "2-S2-2-408510"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostChoice {
    Observed(u32),
}
impl HostChoice {
    fn id(self) -> u32 {
        match self {
            Self::Observed(id) => id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmrModifier {
    Full,
    Half,
    FullOrHalf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmrSource {
    Human,
    Alien,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    CompleteRanges,
    Partial,
    Insufficient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExclusionReason {
    NewPlayer,
    NewRole,
    HostBan,
    AlreadyAlien,
    SelectedHost,
    SpecialRole(u32),
}

impl ExclusionReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::NewPlayer => "新玩家保护",
            Self::NewRole => "新职位保护",
            Self::HostBan => "异形主宰禁选",
            Self::AlreadyAlien => "已经是异形",
            Self::SelectedHost => "已当选异形主宰",
            Self::SpecialRole(_) => "关键职位组合限制",
        }
    }

    fn detail(self) -> String {
        match self {
            Self::NewPlayer => "开局累计游戏场次不足 10，触发新玩家保护，移出初始候选集合。".into(),
            Self::NewRole => "不是新玩家，但当前职位累计出场不足 4 场，触发新职位保护，移出初始候选集合。".into(),
            Self::HostBan => "触发异形主宰禁选规则，移出初始候选集合。".into(),
            Self::AlreadyAlien => "选人前已经属于异形组，不能再次参与选择。".into(),
            Self::SelectedHost => "已在第一轮当选异形主宰，不能再当选最初的异形继任者。".into(),
            Self::SpecialRole(host) => format!("首轮异形主宰是玩家 {host}；异形主宰与本玩家的职位均属于舰长、副官、工程师或安全主管，触发第二轮关键职位组合限制。"),
        }
    }
}

#[derive(Clone)]
pub struct Exclusion {
    pub reason: ExclusionReason,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct PlayerInput {
    pub id: u32,
    pub effective_hmmr: Option<ScoreRange>,
    pub effective_ammr: Option<ScoreRange>,
    pub host_wins: Option<u32>,
    pub successor_wins: Option<u32>,
    pub new_player: Option<bool>,
    pub new_role: Option<bool>,
    pub host_banned: Option<bool>,
    pub already_alien: Option<bool>,
    pub volunteer: [Option<bool>; 2],
    pub deprioritized: Option<bool>,
    pub left_last_game: Option<bool>,
    /// Whether the assigned role is one of 1, 2, 5, or 8.
    pub special_role: Option<bool>,
}

pub struct SelectionInput {
    pub players: Vec<PlayerInput>,
    pub roster_complete: bool,
    pub enabled: Option<bool>,
    /// The source compares two unassigned locals; retain unknown unless proven.
    pub requirement_penalty: Option<bool>,
    pub first_host: Option<HostChoice>,
    /// Inputs include conservative bounds for unverified fixed-point conversion.
    /// This never enables a real-number approximation or a guessed point value.
    pub estimate_fixed_arithmetic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScoreRange {
    pub min: i32,
    pub max: i32,
}

impl ScoreRange {
    pub const fn point(value: i32) -> Self {
        Self {
            min: value,
            max: value,
        }
    }

    pub fn point_value(self) -> Option<i32> {
        (self.min == self.max).then_some(self.min)
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        if self.min > self.max || other.min > other.max {
            return None;
        }
        Some(Self {
            min: self.min.checked_add(other.min)?,
            max: self.max.checked_add(other.max)?,
        })
    }

    pub fn hull(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

impl std::fmt::Display for ScoreRange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.min == self.max {
            write!(formatter, "{}", self.min)
        } else {
            write!(formatter, "{} 至 {}", self.min, self.max)
        }
    }
}

#[derive(Default)]
pub struct ScoreBreakdown {
    pub mmr_penalty: Option<ScoreRange>,
    pub preference_bonus: Option<i32>,
    pub deprioritized_penalty: Option<i32>,
    pub left_penalty: Option<i32>,
    pub new_role_penalty: Option<i32>,
    pub host_ban_penalty: Option<i32>,
    pub new_player_penalty: Option<i32>,
    pub requirement_penalty: Option<i32>,
    pub fixed_range: Option<ScoreRange>,
    pub fixed_score: Option<i32>,
    pub total_range: Option<ScoreRange>,
}

pub struct PlayerScore {
    pub id: u32,
    pub eligible: Option<bool>,
    pub exclusions: Vec<Exclusion>,
    pub used_mmr: Option<ScoreRange>,
    pub mmr_source: Option<MmrSource>,
    pub host_wins: Option<u32>,
    pub successor_wins: Option<u32>,
    pub breakdown: ScoreBreakdown,
    pub reasons: Vec<String>,
}
pub struct RoundAnalysis {
    pub round: u8,
    pub target_mmr: Option<ScoreRange>,
    pub modifier: Option<MmrModifier>,
    pub fallback: Option<bool>,
    pub players: Vec<PlayerScore>,
    pub availability: Availability,
    pub reasons: Vec<String>,
    /// Fixed-part order only. Random rolls remain unknown and may change it.
    pub fixed_order: Option<Vec<u32>>,
}
pub struct SelectionAnalysis {
    pub average_hmmr: Option<ScoreRange>,
    pub rounds: Vec<RoundAnalysis>,
    pub reasons: Vec<String>,
}

fn and(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}
fn not(value: Option<bool>) -> Option<bool> {
    value.map(|value| !value)
}

#[derive(Clone)]
struct Candidate {
    eligible: Option<bool>,
    exclusions: Vec<Exclusion>,
}

impl Candidate {
    fn new(eligible: Option<bool>) -> Self {
        Self {
            eligible,
            exclusions: Vec::new(),
        }
    }

    fn exclude(&mut self, condition: Option<bool>, reason: ExclusionReason) {
        self.eligible = and(self.eligible, not(condition));
        if condition == Some(true) && !self.exclusions.iter().any(|item| item.reason == reason) {
            self.exclusions.push(Exclusion {
                reason,
                detail: reason.detail(),
            });
        }
    }
}

fn initial_candidate(player: &PlayerInput) -> Candidate {
    let mut candidate = Candidate::new(Some(true));
    for (condition, reason) in [
        (player.new_player, ExclusionReason::NewPlayer),
        (player.new_role, ExclusionReason::NewRole),
        (player.host_banned, ExclusionReason::HostBan),
    ] {
        candidate.exclude(condition, reason);
    }
    candidate
}
fn valid_mmr(value: Option<ScoreRange>) -> Option<ScoreRange> {
    value.filter(|value| 0 <= value.min && value.min <= value.max && value.max <= 100_000)
}
fn used_mmr(player: &PlayerInput) -> (Option<ScoreRange>, Option<MmrSource>) {
    let known_wins =
        u64::from(player.host_wins.unwrap_or(0)) + u64::from(player.successor_wins.unwrap_or(0));
    if known_wins >= 5 {
        (valid_mmr(player.effective_ammr), Some(MmrSource::Alien))
    } else if player.host_wins.is_some() && player.successor_wins.is_some() {
        (valid_mmr(player.effective_hmmr), Some(MmrSource::Human))
    } else {
        (
            valid_mmr(player.effective_hmmr)
                .zip(valid_mmr(player.effective_ammr))
                .map(|(human, alien)| human.hull(alien)),
            None,
        )
    }
}
/// Explain the selected MMR branch without treating missing wins as zero.
pub fn mmr_choice_explanation(player: &PlayerScore) -> String {
    let wins = if let (Some(host), Some(successor)) = (player.host_wins, player.successor_wins) {
        let total = u64::from(host) + u64::from(successor);
        let threshold = if total >= 5 { "达到" } else { "未达到" };
        format!("异形主宰 {host} 胜 + 最初的异形继任者 {successor} 胜 = {total} 胜，{threshold} 5 胜门槛。")
    } else {
        let known_minimum: u64 = [player.host_wins, player.successor_wins]
            .into_iter()
            .flatten()
            .map(u64::from)
            .sum();
        let counts = format!(
            "{}，{}",
            win_count("异形主宰", player.host_wins),
            win_count("最初的异形继任者", player.successor_wins)
        );
        if known_minimum >= 5 {
            format!("{counts}；已知至少 {known_minimum} 胜，已达到 5 胜门槛。")
        } else {
            format!("{counts}；合计未确定，无法判断是否达到 5 胜门槛。")
        }
    };
    let source = match player.mmr_source {
        Some(MmrSource::Human) => "HMMR（人类 MMR）",
        Some(MmrSource::Alien) => "AMMR（异形 MMR）",
        None => {
            return player.used_mmr.map_or_else(
                || format!("{wins}本轮 MMR 来源和值均未确定。"),
                |range| {
                    format!("{wins}本轮 MMR 来源未确定，按 HMMR 与 AMMR 的并集包络 {range} 计算。")
                },
            );
        }
    };
    let choice = player.used_mmr.map_or_else(
        || format!("本轮采用 {source} 分支，但该数值未记录或无效。"),
        |value| format!("本轮使用 {source}，评分值范围为 {value}。"),
    );
    format!("{wins}{choice}")
}

fn win_count(label: &str, wins: Option<u32>) -> String {
    wins.map_or_else(
        || format!("{label}胜场未知"),
        |wins| format!("{label} {wins} 胜"),
    )
}

fn average(input: &SelectionInput) -> Option<ScoreRange> {
    if !input.roster_complete || input.players.is_empty() || input.players.len() > 12 {
        return None;
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut sum = ScoreRange::point(0);
    for player in &input.players {
        if !(1..=12).contains(&player.id) || !ids.insert(player.id) {
            return None;
        }
        sum = sum.checked_add(valid_mmr(player.effective_hmmr)?)?;
    }
    let count = i32::try_from(input.players.len()).ok()?;
    Some(ScoreRange {
        min: sum.min / count,
        max: sum.max / count,
    })
}

/// Refill happens before removing already-selected aliens, including in round 2.
fn candidates(
    input: &SelectionInput,
    mut eligibility: Vec<Candidate>,
    host: Option<u32>,
) -> (Vec<Candidate>, Option<bool>) {
    let fallback = if eligibility
        .iter()
        .any(|candidate| candidate.eligible == Some(true))
    {
        Some(false)
    } else if input.roster_complete
        && eligibility
            .iter()
            .all(|candidate| candidate.eligible == Some(false))
    {
        Some(true)
    } else {
        None
    };
    if fallback != Some(false) {
        for candidate in &mut eligibility {
            candidate.eligible = fallback;
            candidate.exclusions.clear();
        }
    }
    for (player, candidate) in input.players.iter().zip(&mut eligibility) {
        candidate.exclude(player.already_alien, ExclusionReason::AlreadyAlien);
        if Some(player.id) == host {
            candidate.exclude(Some(true), ExclusionReason::SelectedHost);
        }
    }
    (eligibility, fallback)
}

fn contribution(flag: Option<bool>, amount: i32) -> Option<i32> {
    flag.map(|flag| if flag { amount } else { 0 })
}
fn contribution_range(value: Option<i32>, amount: i32) -> ScoreRange {
    value.map_or(
        ScoreRange {
            min: amount.min(0),
            max: amount.max(0),
        },
        ScoreRange::point,
    )
}

fn mmr_penalty(used: ScoreRange, target: ScoreRange) -> Option<ScoreRange> {
    let left = used.min.checked_sub(target.max)?;
    let right = used.max.checked_sub(target.min)?;
    let maximum = left.checked_abs()?.max(right.checked_abs()?);
    let minimum = if left <= 0 && right >= 0 {
        0
    } else {
        left.checked_abs()?.min(right.checked_abs()?)
    };
    Some(ScoreRange {
        min: -(maximum / 10),
        max: -(minimum / 10),
    })
}

fn score(
    player: &PlayerInput,
    eligible: Option<bool>,
    target: Option<ScoreRange>,
    round: usize,
    requirement: Option<bool>,
) -> PlayerScore {
    let (used_mmr, mmr_source) = used_mmr(player);
    let mut breakdown = ScoreBreakdown {
        mmr_penalty: used_mmr
            .zip(target)
            .and_then(|(used, target)| mmr_penalty(used, target)),
        preference_bonus: contribution(player.volunteer.get(round).copied().flatten(), 100),
        deprioritized_penalty: contribution(player.deprioritized, -25),
        left_penalty: contribution(player.left_last_game, -100),
        new_role_penalty: contribution(player.new_role, -250),
        host_ban_penalty: contribution(player.host_banned, -5000),
        new_player_penalty: contribution(player.new_player, -5000),
        requirement_penalty: contribution(requirement, -200),
        ..ScoreBreakdown::default()
    };
    let contributions = [
        (breakdown.preference_bonus, 100),
        (breakdown.deprioritized_penalty, -25),
        (breakdown.left_penalty, -100),
        (breakdown.new_role_penalty, -250),
        (breakdown.host_ban_penalty, -5000),
        (breakdown.new_player_penalty, -5000),
        (breakdown.requirement_penalty, -200),
    ];
    if eligible != Some(false) {
        breakdown.fixed_range = breakdown.mmr_penalty.and_then(|mmr| {
            contributions
                .into_iter()
                .try_fold(mmr, |sum, (value, amount)| {
                    sum.checked_add(contribution_range(value, amount))
                })
        });
        breakdown.fixed_score = breakdown.fixed_range.and_then(ScoreRange::point_value);
        breakdown.total_range = breakdown
            .fixed_range
            .and_then(|fixed| fixed.checked_add(ScoreRange { min: 0, max: 50 }));
    }
    let mut reasons = Vec::new();
    match eligible {
        Some(false) => reasons.push("本轮不在候选集合，分项仅供核查，不参与排名。".into()),
        None => reasons
            .push("本轮候选身份未知；评分范围以实际进入候选集合为前提，不能据此认定参与。".into()),
        _ => {}
    }
    if target.is_none() {
        reasons.push("本轮目标 MMR 未知，不能计算 MMR 扣分或总分。".into());
    }
    if used_mmr.is_none() {
        reasons.push("缺少有效 MMR，无法界定本轮使用的 MMR 范围。".into());
    }
    if contributions.iter().any(|(value, _)| value.is_none()) {
        reasons.push("未知报名或处罚项按脚本允许的完整取值区间计入，未补成 0。".into());
    }
    if breakdown
        .fixed_range
        .is_some_and(|range| range.min != range.max)
    {
        reasons.push(
            "固定项仍含输入或定点转换边界；总分为这些边界与 0–50 附加项共同形成的保守包络。".into(),
        );
    }
    PlayerScore {
        id: player.id,
        eligible,
        exclusions: Vec::new(),
        used_mmr,
        mmr_source,
        host_wins: player.host_wins,
        successor_wins: player.successor_wins,
        breakdown,
        reasons,
    }
}

/// Envelope both directed and nearest-integer conversion of nonnegative values.
/// The coefficient bounds include the adjacent Q12 representations of a literal.
fn scaled_bounds(
    value: ScoreRange,
    minimum_numerator: i64,
    maximum_numerator: i64,
    denominator: i64,
) -> Option<ScoreRange> {
    if value.min < 0
        || value.min > value.max
        || minimum_numerator < 0
        || minimum_numerator > maximum_numerator
        || denominator <= 0
    {
        return None;
    }
    let low = i64::from(value.min).checked_mul(minimum_numerator)? / denominator;
    let high = i64::from(value.max)
        .checked_mul(maximum_numerator)?
        .checked_add(denominator.checked_sub(1)?)?
        / denominator;
    Some(ScoreRange {
        min: i32::try_from(low).ok()?,
        max: i32::try_from(high).ok()?,
    })
}

fn target_range(mean: ScoreRange, modifier: MmrModifier) -> Option<ScoreRange> {
    match modifier {
        MmrModifier::Full => Some(mean),
        MmrModifier::Half => scaled_bounds(mean, 1, 1, 2),
        MmrModifier::FullOrHalf => Some(mean.hull(scaled_bounds(mean, 1, 1, 2)?)),
    }
}

fn fixed_order(players: &[PlayerScore]) -> Option<Vec<u32>> {
    let mut order: Vec<_> = players
        .iter()
        .filter(|player| player.eligible == Some(true))
        .map(|player| Some((player.id, player.breakdown.fixed_score?)))
        .collect::<Option<_>>()?;
    order.sort_by_key(|(id, score)| (std::cmp::Reverse(*score), *id));
    Some(order.into_iter().map(|(id, _)| id).collect())
}

fn round(
    input: &SelectionInput,
    number: u8,
    mean: Option<ScoreRange>,
    modifier: Option<MmrModifier>,
    eligibility: Vec<Candidate>,
    fallback: Option<bool>,
) -> RoundAnalysis {
    let target_mmr = mean
        .zip(modifier)
        .and_then(|(mean, modifier)| target_range(mean, modifier));
    let players: Vec<_> = input
        .players
        .iter()
        .zip(eligibility)
        .map(|(player, candidate)| {
            let mut score = score(
                player,
                candidate.eligible,
                target_mmr,
                usize::from(number - 1),
                input.requirement_penalty,
            );
            score.exclusions = candidate.exclusions;
            score
        })
        .collect();
    let complete = input.roster_complete
        && target_mmr.is_some()
        && players.iter().any(|player| player.eligible == Some(true))
        && players.iter().all(|player| {
            player.eligible.is_some()
                && (player.eligible == Some(false) || player.breakdown.total_range.is_some())
        });
    let fixed_order = complete.then(|| fixed_order(&players)).flatten();
    let availability = if complete {
        Availability::CompleteRanges
    } else if target_mmr.is_some() {
        Availability::Partial
    } else {
        Availability::Insufficient
    };
    let mut reasons = vec![
        "脚本对每名候选调用一次 RandomInt(0, 50)；总分为固定项加该整数，最终按最高分选择，同分保留先遍历者。本模型按玩家编号升序处理遍历。".into(),
        "区间比较可以证明必胜、必败或给出胜出条件；区间重叠时不能把固定项排序当作实际结果，也不据此计算概率。".into(),
    ];
    if fallback == Some(true) {
        reasons.push(
            "本轮开始前候选为空，脚本恢复全员后再移除已选异形；原筛选者可能重新入选并承担扣分。"
                .into(),
        );
    }
    if fallback.is_none() {
        reasons.push("候选是否为空尚不确定，无法确认是否触发全员回退。".into());
    }
    if input.requirement_penalty.is_none() {
        reasons.push(
            "额外条件未确定时按全体候选共享的 0 或 −200 计入包络；该共同平移不会改变实际排序。"
                .into(),
        );
    }
    if input.estimate_fixed_arithmetic {
        reasons.push("有效 MMR 与定点转换使用整数上下界传播；未验证的客户端取整保留在包络中，不以实数近似替代实际值。".into());
    }
    if modifier == Some(MmrModifier::FullOrHalf) {
        reasons.push(
            "第二轮倍率比较横跨定点边界，目标范围同时覆盖均值 × 1 与均值 × 0.5 两种分支。".into(),
        );
    }
    RoundAnalysis {
        round: number,
        target_mmr,
        modifier,
        fallback,
        players,
        availability,
        reasons,
        fixed_order,
    }
}

fn host_score<'a>(input: &SelectionInput, first: &'a RoundAnalysis) -> Option<&'a PlayerScore> {
    let id = input.first_host?.id();
    first.players.iter().find(|player| player.id == id)
}

fn second_candidates(
    input: &SelectionInput,
    first: &RoundAnalysis,
) -> (Vec<Candidate>, Option<bool>) {
    let Some(host) = input
        .first_host
        .map(HostChoice::id)
        .and_then(|id| input.players.iter().find(|player| player.id == id))
    else {
        return candidates(input, vec![Candidate::new(None); input.players.len()], None);
    };
    if host_score(input, first).is_some_and(|player| player.eligible == Some(false)) {
        return candidates(
            input,
            vec![Candidate::new(None); input.players.len()],
            Some(host.id),
        );
    }
    let eligibility = input
        .players
        .iter()
        .zip(&first.players)
        .map(|(player, first)| {
            let mut candidate = Candidate {
                eligible: first.eligible,
                exclusions: first.exclusions.clone(),
            };
            if player.id == host.id {
                candidate.exclude(Some(true), ExclusionReason::SelectedHost);
            } else {
                candidate.exclude(
                    and(host.special_role, player.special_role),
                    ExclusionReason::SpecialRole(host.id),
                );
            }
            candidate
        })
        .collect();
    candidates(input, eligibility, Some(host.id))
}

fn second_modifier(
    input: &SelectionInput,
    first: &RoundAnalysis,
    mean: Option<ScoreRange>,
) -> Option<MmrModifier> {
    if host_score(input, first)?.eligible == Some(false) {
        return None;
    }
    // This model assumes ascending-ID PlayerGroupNextPlayer traversal.
    // The source leaves usedMMR at the LAST scored candidate, not the host.
    let last = first
        .players
        .iter()
        .filter(|player| player.eligible != Some(false))
        .min_by_key(|player| std::cmp::Reverse(player.id))?;
    let mean = mean?;
    if last.eligible != Some(true) {
        return Some(MmrModifier::FullOrHalf);
    }
    let Some(used) = last.used_mmr else {
        return Some(MmrModifier::FullOrHalf);
    };
    // Enclose both adjacent Q12 encodings of 0.8, then enclose conversion
    // to an integer. Only disjoint comparison bounds select one branch.
    let threshold = scaled_bounds(used, 3276, 3277, 4096)?;
    Some(if mean.max < threshold.min {
        MmrModifier::Half
    } else if mean.min >= threshold.max {
        MmrModifier::Full
    } else {
        MmrModifier::FullOrHalf
    })
}

pub fn analyze(input: &SelectionInput) -> SelectionAnalysis {
    let mut reasons = Vec::new();
    if input.enabled != Some(true) {
        reasons.push(
            if input.enabled == Some(false) {
                "该模式不执行这段异形选人逻辑。"
            } else {
                "游戏模式尚未确认，无法确认是否执行这段选人逻辑。"
            }
            .into(),
        );
        return SelectionAnalysis {
            average_hmmr: None,
            rounds: Vec::new(),
            reasons,
        };
    }
    let average_hmmr = average(input);
    if average_hmmr.is_none() {
        reasons
            .push("缺少完整参与者名单或至少一人的有效 HMMR；不计算全房平均、目标分与排名。".into());
    }
    let (eligible, fallback) = candidates(
        input,
        input.players.iter().map(initial_candidate).collect(),
        None,
    );
    let first = round(
        input,
        1,
        average_hmmr,
        Some(MmrModifier::Full),
        eligible,
        fallback,
    );
    let mut rounds = Vec::new();
    if input.players.len() >= 10 || !input.roster_complete {
        let host_conflict = input.first_host.is_some_and(|host| {
            ranges::compare(&first, host.id(), Some(host.id())).verdict == ranges::Verdict::Conflict
        });
        let (modifier, (eligible, fallback)) = if host_conflict {
            (
                None,
                candidates(
                    input,
                    vec![Candidate::new(None); input.players.len()],
                    input.first_host.map(HostChoice::id),
                ),
            )
        } else {
            (
                second_modifier(input, &first, average_hmmr),
                second_candidates(input, &first),
            )
        };
        let mut second = round(input, 2, average_hmmr, modifier, eligible, fallback);
        if host_conflict {
            second.reasons.push(
                "首轮模型与录像冲突：已观察异形主宰被候选规则排除或其最高可能分仍必败；第二轮不推导确定倍率、评分或候选因果。".into(),
            );
        } else if input.first_host.is_some()
            && host_score(input, &first).is_none_or(|player| player.eligible == Some(false))
        {
            second.reasons.push(
                "首轮异形主宰不在已知候选中或参与者名单内，输入事实互相冲突；第二轮不提供确定评分。"
                    .into(),
            );
        }
        if input.first_host.is_none() {
            second
                .reasons
                .push("尚无已证实的首轮异形主宰，第二轮候选变化未知。".into());
        }
        if modifier.is_none() {
            second
                .reasons
                .push("第二轮倍率取决于首轮最后遍历候选的 MMR；条件或固定点比较尚未确定。".into());
        }

        rounds.push(first);
        rounds.push(second);
    } else {
        rounds.push(first);
    }
    SelectionAnalysis {
        average_hmmr,
        rounds,
        reasons,
    }
}

#[path = "selection_ranges.rs"]
pub mod ranges;

#[cfg(test)]
#[path = "alien_selection_tests.rs"]
mod tests;
