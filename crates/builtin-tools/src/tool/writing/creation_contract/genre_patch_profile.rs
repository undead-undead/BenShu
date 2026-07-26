use super::*;

#[derive(Debug, Clone)]
pub(crate) struct GenrePatchProfile {
    pub(crate) profile: longform_policy::FictionGenreProfile,
    pub(crate) required_patch_fields: BTreeMap<String, PatchFieldStrength>,
    pub(crate) prompt_hints: Vec<String>,
    pub(crate) quality_axes: Vec<String>,
}

impl GenrePatchProfile {
    pub(crate) fn from_draft(draft: &SessionCreationDraftState, user_message: &str) -> Self {
        let profile = longform_policy::fiction_genre_profile(user_message, Some(&draft.genre));
        let required_patch_fields =
            longform_policy::fiction_contract_field_requirements(&draft.genre)
                .into_iter()
                .map(|(key, value)| (key, PatchFieldStrength::from_policy_value(&value)))
                .collect::<BTreeMap<_, _>>();
        let mut prompt_hints = Vec::new();
        let mut quality_axes = vec![
            "书名必须来自结局、主线、世界观意象或关键事件".to_string(),
            "角色权威表必须保持唯一主角和关系引用一致".to_string(),
            "分卷/章节目标必须有不可逆变化".to_string(),
        ];
        match profile {
            longform_policy::FictionGenreProfile::Fantasy
            | longform_policy::FictionGenreProfile::Xianxia => {
                prompt_hints
                    .push("玄幻/仙侠：力量秩序、资源代价、阶层压力和防膨胀必须具体".to_string());
                quality_axes.push("成长体系不能无代价膨胀".to_string());
                quality_axes.push("资源/货币/法则必须约束主角行动".to_string());
            }
            longform_policy::FictionGenreProfile::ScienceFiction => {
                prompt_hints.push(
                    "科幻：技术边界、资源/能源约束、制度冲突、空间或时间尺度必须具体".to_string(),
                );
                quality_axes.push("技术或权限进阶不能万能化".to_string());
                quality_axes.push("资源、通信、航行或制度边界必须产生叙事约束".to_string());
            }
            longform_policy::FictionGenreProfile::Romance => {
                prompt_hints
                    .push("言情/关系：情绪承诺、关系阶段、现实压力和选择代价必须具体".to_string());
                quality_axes.push("关系线必须有起点、冲突、选择代价和终局状态".to_string());
                quality_axes.push("情绪推进不能被外部设定吞掉".to_string());
            }
            longform_policy::FictionGenreProfile::Mystery => {
                prompt_hints.push(
                    "悬疑/推理：谜面、线索公平性、知情层级、误导边界和揭示节奏必须具体".to_string(),
                );
                quality_axes.push("核心真相必须由已登记线索支撑".to_string());
                quality_axes.push("读者与角色的知情差必须可追踪".to_string());
            }
            longform_policy::FictionGenreProfile::General => {
                prompt_hints.push(
                    "泛类型：只补通用创作字段，类型专属字段按用户题材自然需要生成".to_string(),
                );
                quality_axes.push("不要硬套修炼、恋爱或科幻模板".to_string());
            }
        }
        Self {
            profile,
            required_patch_fields,
            prompt_hints,
            quality_axes,
        }
    }

    pub(crate) fn prompt_hint_text(&self) -> String {
        let field_text = self
            .required_patch_fields
            .iter()
            .map(|(key, strength)| format!("{key}={}", strength.as_prompt_label()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{}；字段强度：{}；质量轴：{}",
            self.prompt_hints.join("；"),
            field_text,
            self.quality_axes.join("；")
        )
    }

    pub(crate) fn governance_schema_suffix(&self) -> &'static str {
        match self.profile {
            longform_policy::FictionGenreProfile::Fantasy
            | longform_policy::FictionGenreProfile::Xianxia => {
                r#","resource_economy":{"currency":"资源/货币","value_scale":"价值尺度","resource_types":["资源类型"],"scarcity_rules":["稀缺规则"]},"power_progression":{"system_name":"成长体系","levels":["层级"],"advancement_costs":["晋升代价"],"anti_power_creep_rules":["防膨胀规则"]},"social_order":{"institutions":["机构"],"rank_system":"阶层/等级","authority_conflicts":["权力冲突"]},"geography_model":{"important_locations":["关键地点"],"travel_constraints":["移动约束"]}"#
            }
            longform_policy::FictionGenreProfile::ScienceFiction => {
                r#","resource_economy":{"currency":"能源/算力/信用等资源","value_scale":"技术或资源价值尺度","resource_types":["资源类型"],"scarcity_rules":["稀缺规则"]},"power_progression":{"system_name":"技术/权限/能力进阶体系","levels":["阶段"],"advancement_costs":["升级代价"],"anti_power_creep_rules":["防技术万能规则"]},"social_order":{"institutions":["机构"],"rank_system":"权限/阶层/组织结构","authority_conflicts":["权力冲突"]},"time_model":{"story_start_time":"开场时间","deadline_events":["期限事件"],"time_skip_rules":["时间跳跃规则"]},"geography_model":{"important_locations":["关键地点"],"travel_constraints":["移动/航行/通信约束"]}"#
            }
            longform_policy::FictionGenreProfile::Romance => {
                r#","social_order":{"institutions":["现实机构/家庭/职业环境"],"rank_system":"关系或社会压力结构","authority_conflicts":["现实压力/价值冲突"]},"time_model":{"story_start_time":"开场时间","deadline_events":["关键期限事件"],"time_skip_rules":["时间跳跃规则"]}"#
            }
            longform_policy::FictionGenreProfile::Mystery => {
                r#","artifact_ledger":[{"name":"关键线索","role":"线索/证据/误导","introduced_in":"计划出现位置","current_holder":"持有者","state":"当前状态","symbolic_meaning":"叙事意义","payoff_target":"揭示窗口"}],"reveal_schedule":[{"secret_id":"核心秘密","reader_knows":false,"character_knowers":[],"planned_reveal_window":"揭示窗口","status":"planned"}],"time_model":{"story_start_time":"开场时间","deadline_events":["调查期限"],"time_skip_rules":["时间跳跃规则"]}"#
            }
            longform_policy::FictionGenreProfile::General => {
                r#","resource_economy":{"currency":"故事中真正重要的资源，如无则写现实资源","value_scale":"价值尺度","resource_types":["资源类型"],"scarcity_rules":["稀缺规则"]},"social_order":{"institutions":["机构/关系网络"],"rank_system":"社会结构","authority_conflicts":["权力或规则冲突"]},"time_model":{"story_start_time":"开场时间","deadline_events":["期限事件"],"time_skip_rules":["时间跳跃规则"]}"#
            }
        }
    }
}
