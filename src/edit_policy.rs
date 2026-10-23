use std::collections::BTreeMap;

use crate::model::FIELDS;

// Numeric-field permissions are fixed at compile time; both editions may edit FA/Name.
pub const ROLE_BAN_ONLY: bool = cfg!(feature = "role-ban-only");

pub fn is_role_ban(key: &str) -> bool {
    matches!(
        key,
        "lv_modRoleBan[1]"
            | "lv_modRoleBan[2]"
            | "lv_modRoleBan[3]"
            | "lv_modRoleBan[4]"
            | "lv_modRoleBan[5]"
            | "lv_modRoleBan[6]"
            | "lv_modRoleBan[7]"
            | "lv_modRoleBan[8]"
            | "lv_modRoleBan[9]"
            | "lv_modRoleBan[10]"
            | "lv_modRoleBan[11]"
            | "lv_modRoleBan[12]"
    )
}

pub fn can_edit(key: &str) -> bool {
    if ROLE_BAN_ONLY {
        is_role_ban(key)
    } else {
        FIELDS.iter().any(|field| field.key == key)
    }
}

pub fn validate_changes(
    original: &BTreeMap<String, u64>,
    edited: &BTreeMap<String, u64>,
) -> Result<(), String> {
    if !original.keys().eq(edited.keys()) {
        return Err("修改字段与源存档不一致，不能添加或遗漏字段。".into());
    }
    for (key, value) in edited {
        if original.get(key) != Some(value) && !can_edit(key) {
            return Err(format!(
                "当前版本不允许修改字段 {key}；普通版允许修改名字和角色 1–12 的封禁位，其他统计字段只读。"
            ));
        }
    }
    Ok(())
}
