use FieldKind::{Flag, Identity, Role, Statistic};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Statistic,
    Flag,
    Role,
    Identity,
}

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub since: u32,
}

impl Field {
    const fn new(key: &'static str, label: &'static str, kind: FieldKind, since: u32) -> Self {
        Self {
            key,
            label,
            kind,
            since,
        }
    }

    pub fn maximum(&self, version: u32) -> u64 {
        match self.kind {
            FieldKind::Flag => 1,
            FieldKind::Identity => 100_000_000,
            _ if version >= 6 => 9_999,
            _ => 99_999,
        }
    }
}

pub const FIELDS: &[Field] = &[
    Field::new("lv_statsGamesPlayed", "游戏次数", Statistic, 0),
    Field::new("lv_statsGamesSpawned", "异形子代", Statistic, 0),
    Field::new("lv_statsGamesHost", "主宰局数", Statistic, 0),
    Field::new("lv_statsGamesWon", "游戏胜利", Statistic, 0),
    Field::new("lv_statsGamesWonHuman", "人类游戏胜利", Statistic, 0),
    Field::new("lv_statsGamesWonSpawn", "异形子代赢得游戏", Statistic, 0),
    Field::new("lv_statsGamesWonHost", "游戏获胜异形主宰", Statistic, 0),
    Field::new("lv_statsGamesWonAdvisor", "游戏胜利指导", Statistic, 0),
    Field::new("lv_statsGamesDeadHuman", "人类死亡", Statistic, 0),
    Field::new("lv_statsGamesDeadSpawn", "异形子代死亡·", Statistic, 0),
    Field::new("lv_statsGamesDeadHost", "异形主宰死亡", Statistic, 0),
    Field::new("lv_statsGamesLost", "游戏失败", Statistic, 0),
    Field::new("lv_statsAliensKilled", "异形被杀", Statistic, 0),
    Field::new("lv_statsHumansKilled", "人类死亡", Statistic, 0),
    Field::new("lv_statsTeamkills", "队友杀手", Statistic, 0),
    Field::new("lv_statsPlayersSpawned", "玩家子代", Statistic, 0),
    Field::new("lv_statsGamesLeft", "秒退次数", Statistic, 1),
    Field::new("lv_statsVetWin", "老兵游戏胜利场次", Statistic, 1),
    Field::new("lv_autoModTrollTeamkill", "队友杀手处罚计数", Statistic, 1),
    Field::new("lv_autoModTrollAlien", "异形违规计数", Statistic, 1),
    Field::new("lv_autoModTrollSabotage", "破坏行为计数", Statistic, 1),
    Field::new(
        "lv_modLeftAfterSpawn",
        "变成子代后退出游戏剩余对局数",
        Statistic,
        1,
    ),
    Field::new(
        "lv_autoModGamesdiedashostafterdoingnothing",
        "异形主宰无行动死亡计数",
        Statistic,
        1,
    ),
    Field::new(
        "lv_autoModTemphostbangames",
        "临时异形主宰限制场次",
        Statistic,
        1,
    ),
    Field::new("lv_alienSelectionHMMR", "HMMR", Statistic, 1),
    Field::new("lv_alienSelectionAMMR", "AMMR", Statistic, 1),
    Field::new("lv_modTeamkiller", "队友杀手标记", Flag, 0),
    Field::new("lv_modAlienBan", "禁止竞选异形", Flag, 0),
    Field::new("lv_modSelfer", "自私自利者标记", Flag, 0),
    Field::new("lv_modFake", "赝品标记", Flag, 1),
    Field::new("lv_modSaboteur", "暴民或新手标记", Flag, 1),
    Field::new("lv_modTeamkiller2", "队友杀手 2 标记", Flag, 1),
    Field::new("lv_modNameBan", "名称禁用", Flag, 1),
    Field::new("lv_modLeftLastGame", "上一盘退出游戏", Flag, 1),
    Field::new("lv_modBan", "封禁标记", Flag, 1),
    Field::new("lv_modRoleBan[1]", "舰长限制", Flag, 3),
    Field::new("lv_modRoleBan[2]", "副官限制", Flag, 3),
    Field::new("lv_modRoleBan[3]", "武器官限制", Flag, 3),
    Field::new("lv_modRoleBan[4]", "机械师限制", Flag, 3),
    Field::new("lv_modRoleBan[5]", "工程师限制", Flag, 3),
    Field::new("lv_modRoleBan[6]", "技术官限制", Flag, 3),
    Field::new("lv_modRoleBan[7]", "医生限制", Flag, 3),
    Field::new("lv_modRoleBan[8]", "安全主管限制", Flag, 3),
    Field::new("lv_modRoleBan[9]", "飞行员限制", Flag, 3),
    Field::new("lv_modRoleBan[10]", "生物学家限制", Flag, 3),
    Field::new("lv_modRoleBan[11]", "列兵限制（本版排除无效）", Flag, 3),
    Field::new("lv_modRoleBan[12]", "预留角色12（未分配）限制", Flag, 3),
    Field::new("lv_modTeamkiller3", "队友杀手 3 标记", Flag, 3),
    Field::new("lv_modTKScoreReset", "MMR 重置标记", Flag, 3),
    Field::new("lv_flagSpawnDepriortized", "异形优先级降低", Flag, 3),
    Field::new("lv_screenShakeDisabled", "屏幕晃动禁用", Flag, 3),
    Field::new("lv_modHostBan", "异形主宰限制", Flag, 3),
    Field::new("lv_xExtraFlag6", "预留标志 6", Flag, 3),
    Field::new("lv_xExtraFlag7", "预留标志 7", Flag, 3),
    Field::new("lv_xExtraFlag8", "预留标志 8", Flag, 3),
    Field::new("lv_xExtraFlag9", "预留标志 9", Flag, 3),
    Field::new("lv_xExtraFlag10", "预留标志 10", Flag, 3),
    Field::new("lv_statsGamesPlayedAsRole[1]", "舰长出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[2]", "副官出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[3]", "武器官出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[4]", "机械师出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[5]", "工程师出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[6]", "技术官出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[7]", "医生出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[8]", "安全主管出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[9]", "飞行员出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[10]", "生物学家出场次数", Role, 4),
    Field::new("lv_statsGamesPlayedAsRole[11]", "列兵出场次数", Role, 4),
    Field::new(
        "lv_statsGamesPlayedAsRole[12]",
        "预留角色12（未分配）出场次数",
        Role,
        4,
    ),
    Field::new("player_id", "玩家 ID", Identity, 0),
];

#[derive(Clone)]
pub struct BankReport {
    pub player_name: Option<String>,
    pub account_id: String,
    pub path: std::path::PathBuf,
    pub server_id: Option<u32>,
    pub bank_version: u32,
    pub fields: std::collections::BTreeMap<String, u64>,
    pub decode_note: String,
    pub signature_present: bool,
    pub source_text: String,
}

impl BankReport {
    pub fn metric(&self, key: &str) -> Option<u64> {
        self.fields.get(key).copied()
    }
}
