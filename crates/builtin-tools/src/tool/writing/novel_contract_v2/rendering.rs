use super::{EmotionalStateLedgerEntry, NovelContractV2, RelationshipLedgerEntry};

pub(crate) fn summary_lines(contract: &NovelContractV2) -> Vec<String> {
    let mut lines = Vec::new();
    push_line(
        &mut lines,
        "情感承诺",
        &contract.emotional_contract.emotional_promise,
    );
    if !contract.emotional_contract.relief_beats.is_empty() {
        lines.push(format!(
            "节奏缓冲：{}",
            contract.emotional_contract.relief_beats.join("；")
        ));
    }
    push_line(
        &mut lines,
        "主要压力",
        &contract.antagonist_pressure.primary_pressure,
    );
    push_line(
        &mut lines,
        "资源尺度",
        &contract.resource_economy.value_scale,
    );
    push_line(
        &mut lines,
        "成长体系",
        &contract.power_progression.system_name,
    );
    push_line(&mut lines, "社会秩序", &contract.social_order.rank_system);
    push_line(&mut lines, "时间口径", &contract.time_model.calendar);
    push_line(&mut lines, "叙事视角", &contract.narration_contract.pov);
    push_line(&mut lines, "读者期待", &contract.reader_promise.core_hook);
    push_line(
        &mut lines,
        "场景配比",
        &contract.scene_type_mix.balance_rule,
    );
    if !contract.chapter_ending_rotation.planned_rotation.is_empty() {
        lines.push(format!(
            "章尾轮换：{}",
            contract.chapter_ending_rotation.planned_rotation.join("；")
        ));
    }
    if !contract.conflict_pressure_curve.global_curve.is_empty() {
        lines.push(format!(
            "冲突曲线：{} 段",
            contract.conflict_pressure_curve.global_curve.len()
        ));
    }
    for (label, count) in [
        ("角色声音", contract.character_voice_ledger.len()),
        ("主题母题", contract.motif_ledger.len()),
        ("揭示节奏", contract.reveal_schedule.len()),
        (
            "关系互动配额",
            contract.relationship_interaction_quotas.len(),
        ),
    ] {
        if count > 0 {
            lines.push(format!("{label}：{count} 条"));
        }
    }
    if !contract.relationship_ledger.is_empty() {
        lines.push(format!(
            "主要关系：{} 条",
            contract.relationship_ledger.len()
        ));
        let visible = contract
            .relationship_ledger
            .iter()
            .take(3)
            .filter_map(relationship_summary_line)
            .collect::<Vec<_>>();
        if !visible.is_empty() {
            lines.push(format!("关系状态：{}", visible.join("；")));
        }
    }
    let emotions = contract
        .emotional_state_ledger
        .iter()
        .take(3)
        .filter_map(emotional_state_summary_line)
        .collect::<Vec<_>>();
    if !emotions.is_empty() {
        lines.push(format!("情绪状态：{}", emotions.join("；")));
    }
    if !contract.payoff_matrix.is_empty() {
        lines.push(format!("兑现矩阵：{} 条", contract.payoff_matrix.len()));
    }
    lines
}

fn relationship_summary_line(relation: &RelationshipLedgerEntry) -> Option<String> {
    let characters = relation
        .characters
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if characters.is_empty() {
        return None;
    }
    let state = [
        relation.stage.as_str(),
        relation.current_state.as_str(),
        relation.next_expected_stage.as_str(),
    ]
    .into_iter()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .unwrap_or("未记录");
    Some(format!("{characters}：{state}"))
}

fn emotional_state_summary_line(entry: &EmotionalStateLedgerEntry) -> Option<String> {
    let character = entry.character.trim();
    if character.is_empty() {
        return None;
    }
    let state = [
        entry.current_emotion.as_str(),
        entry.pressure.as_str(),
        entry.expected_next_shift.as_str(),
    ]
    .into_iter()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .unwrap_or("未记录");
    Some(format!("{character}：{state}"))
}

fn push_line(lines: &mut Vec<String>, label: &str, value: &str) {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        lines.push(format!("{label}：{trimmed}"));
    }
}
