use super::issue::{
    ClassifiedContractIssue, ContractIssueKind, ContractIssueList, ContractIssueSet,
};
#[cfg(test)]
use super::issue::{ContractIssue, ContractIssueEvidence};
use super::staged_prompts::ContractCompletionStage;
use super::*;

const STORY_BLUEPRINT_BOUNDARY: &str = "只输出小说故事设定、人物、剧情、节奏和质量约束字段；不要把任务理解成法律文书、商业协议、交付文件或权利义务文件；不要输出非故事设定的参与方身份栏、权利义务栏、付款栏、签署栏、期限安排或知识产权安排。";
const INITIAL_CHARACTER_PATCH_EXAMPLE: &str = r#"{"patch_type":"character_patch","characters":[{"canonical_name":"主角槽位","role":"主角","desire":"欲望","fear":"恐惧","bottom_line":"底线","arc_start":"弧线起点","arc_end":"弧线终点","planned_entry":"计划登场阶段","planned_exit":"计划离场或终局状态"},{"canonical_name":"同伴槽位","role":"关键同伴","desire":"欲望","fear":"恐惧","bottom_line":"底线","arc_start":"弧线起点","arc_end":"弧线终点","planned_entry":"计划登场阶段","planned_exit":"计划离场或终局状态"},{"canonical_name":"对手槽位","role":"关键对手","desire":"欲望","fear":"恐惧","bottom_line":"底线","arc_start":"弧线起点","arc_end":"弧线终点","planned_entry":"计划登场阶段","planned_exit":"计划离场或终局状态"}]}"#;

pub(super) fn initial_contract_batch_prompt(
    draft: &SessionCreationDraftState,
    user_message: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
) -> String {
    let profile_hint = genre_patch_prompt_hint(draft, user_message);
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图初始阶段：完整 typed batch\n\
用户正在确定小说故事蓝图。请一次输出紧凑、可确认、可机读的初始合同字段包。{STORY_BLUEPRINT_BOUNDARY}不要写正文，不要解释，不输出 Markdown 代码块。\n\
初始字段包必须同时建立四个现有 typed owner：故事骨架、角色权威、剧情规划和治理约束；不要只返回其中一个 patch。后续有限 semantic patch 只用于修复残缺字段，不负责从零补完其他 owner。\n\
用户未指定的创意字段由你根据题材补全，不得输出“待补”“待定”“未指定”“暂无”“placeholder”或同义占位。\n\
书名先给 3 个来自当前故事物件、地点、制度、事件、关系或终局变化的候选，再选 canonical_title；title.rationale 必须指向具体故事证据。书名与人名不附英文译名或拼音，不要混入韩文/日文/英文括注。\n\
角色表必须恰好 1 个主角，并至少包含 1 个关系对象/盟友/导师和 1 个关键对手/反派/压力源。每个角色都要有完整的欲望、恐惧、带具体对象的底线、弧线起点和弧线终点。姓名只是模型候选，系统会在锁定前统一执行本地命名治理并同步改写所有故事字段。\n\
用户没有明确指定主角性别时，主角 role 必须写“主角”，不得擅自写“男主”或“女主”；用户明确指定时才保留相应性别角色，并确保全部故事字段的身份指代一致。\n\
剧情规划初始包只输出 2 个 volumes 和从第 1 章起连续编号的 3 个 near_chapters；近期章节只是开篇窗口，不得提前完成全书终局、主冲突总解决或主角弧线终点，第三章必须留下明确后续主线债务。每卷 objective 与 ending_change、每章 goal 与 expected_turn 必须分别描述目标和不可逆变化；每个 expected_turn 只写一个点名受影响人物或权威实体、能由章末一段连续正文证明的完整结果，不得串联互不依赖的多个变化。\n\
治理约束至少包含核心主题、3 条可执行世界规则、叙事风格和必须避免；世界规则必须写清代价、限制、稀缺条件或失败后果，不能重复世界意象。\n\
为避免本地模型长 JSON 尾部截断，本轮使用现有中文 field-pack 兼容边界，不输出 JSON。严格按故事骨架、角色权威、治理约束、剧情规划的顺序输出；不要增加模板外字段。每个角色锚点、规则、卷目标和章节事件各用一个短句，全书大纲摘要不超过 120 个中文字。\n\
硬性数值必须原样保留：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。总字数可以是任意正整数；每章档位只能是 2500 或 5000。\n\
语言边界：{language_boundary}\n\
题材字段提示：{profile_hint}\n\
用户最新要求：{}\n\n\
系统会把这些字段归位到现有 typed batch，并由 typed contract gate 审核后再允许用户确认。第一行必须原样输出 `patch_type: contract_batch`，用于选择现有 typed patch 入口；随后只输出以下中文字段包，不输出字段名解释、前言或后记：\n\
patch_type: contract_batch\n\
书名：作品名\n\
书名候选：候选一（关键物件，故事依据）；候选二（地点事件，故事依据）；候选三（结局变化，故事依据）\n\
书名理由：最终书名如何来自具体故事证据\n\
题材：具体题材\n\
简述：一句话故事方向\n\
总字数：{exact_total_units}\n\
每章字数：{exact_chapter_unit}\n\
故事前提：完整故事前提\n\
终局方向：主角终局行动与直接结果\n\
终局状态：行动完成后的不可逆状态\n\
主角弧线：从起点到终点的变化\n\
世界观意象：本故事独有意象\n\
总主线因果链：连续因果短句\n\
角色权威表：\n\
姓名：主角候选名，角色：主角，欲望：具体欲望，恐惧：具体恐惧，底线：不以具体对象换取目标，弧线起点：起点，弧线终点：终点。\n\
姓名：关系角色候选名，角色：关键关系对象，欲望：具体欲望，恐惧：具体恐惧，底线：必须守住具体对象，弧线起点：起点，弧线终点：终点。\n\
姓名：对手候选名，角色：关键对手，欲望：具体欲望，恐惧：具体恐惧，底线：绝不放弃具体控制对象，弧线起点：起点，弧线终点：终点。\n\
核心主题：主题一；主题二\n\
世界规则：规则与代价一；规则与限制二；规则与失败后果三\n\
叙事风格：具体叙事风格\n\
必须避免：角色无解释改名；能力无代价突破；提前完成终局\n\
全书大纲：不超过120字的阶段摘要，终局只在最后一卷完成。\n\
分卷规划：紧接着输出 2 行 `第N卷《卷名》：本卷目标：事件；卷尾变化：结果`。事件与结果必须写入上方已生成的具名角色、物件、地点或制度及其状态变化；禁止复述“阶段证据”“主线债务”“权威终局”“不可逆变化”等说明词。\n\
近期章节包：再输出从第1章开始连续的 3 行 `第N章《章名》：本章目标：事件；预期转折：结果`。每行都必须写入上方已生成的具名角色和具体事件结果；第三章用尚未解决的具体威胁、损失、秘密或行动自然保留后续剧情，禁止输出模板说明词。",
        user_message.trim()
    )
}

pub(super) fn final_prompt_from_patch_completion(
    draft: &SessionCreationDraftState,
    user_message: &str,
    stage: ContractCompletionStage,
    issues: &ContractIssueList,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
) -> Option<String> {
    match stage {
        ContractCompletionStage::Skeleton => Some(skeleton_patch_prompt(
            draft,
            user_message,
            issues,
            stable_anchor,
            exact_total_units,
            exact_chapter_unit,
            expected_chapters,
            language_boundary,
        )),
        ContractCompletionStage::Characters => Some(character_patch_prompt(
            draft,
            user_message,
            issues,
            stable_anchor,
            exact_total_units,
            exact_chapter_unit,
            expected_chapters,
            language_boundary,
        )),
        ContractCompletionStage::Plot => Some(plot_patch_prompt(
            draft,
            user_message,
            issues,
            stable_anchor,
            exact_total_units,
            exact_chapter_unit,
            expected_chapters,
            language_boundary,
        )),
        ContractCompletionStage::Governance => Some(governance_patch_prompt(
            draft,
            user_message,
            issues,
            stable_anchor,
            exact_total_units,
            exact_chapter_unit,
            expected_chapters,
            language_boundary,
        )),
    }
}

fn skeleton_patch_prompt(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issues: &ContractIssueList,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
) -> String {
    let profile_hint = genre_patch_prompt_hint(draft, user_message);
    let issue_focus = contract_patch_issue_focus_text(&stage_relevant_contract_issues(
        ContractCompletionStage::Skeleton,
        issues,
    ));
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图补齐阶段：Skeleton typed patch\n\
用户正在确定小说故事蓝图，当前只需要补齐结构化创作字段。请输出紧凑、可确认、可机读的“故事蓝图字段包”。{STORY_BLUEPRINT_BOUNDARY}你只生成字段补丁，不写正文，不解释，不输出 Markdown 代码块。\n\
本阶段只补故事骨架和书名：题材、简述、故事前提、终局方向、终局状态、主角弧线、世界观意象、总主线因果链、书名候选和书名理由。\n\
角色权威表若已用“女主”或“男主”锁定主角身份，简述、故事前提、主角弧线和终局字段对该角色的指代必须一致；质量门指出冲突时必须重写冲突的故事字段，不得原样返回。\n\
终局方向必须写清主角采取的具体行动和直接结果；终局状态必须非空，并写成行动完成后已经不可逆改变的制度、关系、身份、资源归属或公共状态，不能只写情绪、愿望或“失败/成功”。\n\
	生成顺序必须是：先终局和主线，再世界规则和关键意象，再生成恰好 3 个彼此不同的候选书名，最后从候选里定名；每个候选 rationale 控制在 18 到 35 个中文字，title.rationale 控制在 30 到 50 个中文字，并说明书名如何来自结局、主线、世界规则或关键事件。\n\
	候选书名必须来自当前合同里的关键物件、地点、制度、事件、人物关系或结局变化；文字必须完整、自然、无乱码和残句，不以营销吸引力作为质量门。\n\
用户没有指定的创作字段由你根据题材和已有锚点补全，不得输出“待补”“待定”“未指定”“暂无”“placeholder”或同义占位。\n\
不要启动正式写作，不要输出角色详情、章节正文、长篇大纲或治理字段。\n\n\
稳定锚点：\n{stable_anchor}\n\n\
当前质量门缺口：{issue_focus}\n\n\
用户最新要求：{}\n\n\
硬性数值：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。\n\
语言边界：{language_boundary}\n\
题材字段提示：{profile_hint}\n\n\
系统会根据 JSON 渲染面板创作草案，也会根据结构化故事蓝图渲染面板创作草案；当前阶段未补齐的字段会由后续阶段继续补齐。\n\
优先输出这个 JSON 补丁；如果本地模型无法稳定 JSON，也必须用同名中文字段逐行输出。只输出一个完整 JSON 对象，字段示例：\n\
{{\n\
  \"patch_type\": \"skeleton_patch\",\n\
	  \"title\": {{\"canonical_title\": \"作品名\", \"candidates\": [{{\"title\":\"候选1\",\"hook_type\":\"关键物件\",\"rationale\":\"来自终局或主线的简短理由\"}},{{\"title\":\"候选2\",\"hook_type\":\"地点事件\",\"rationale\":\"来自故事证据的简短理由\"}},{{\"title\":\"候选3\",\"hook_type\":\"结局变化\",\"rationale\":\"来自结局变化的简短理由\"}}], \"rationale\": \"最终书名来自终局、主线、世界规则或关键事件的简短理由\"}},\n\
  \"genre\": \"题材\",\n\
  \"brief\": \"一句话故事方向\",\n\
  \"target_units\": 50000,\n\
  \"chapter_unit_target\": 2500,\n\
  \"max_chapters_per_turn\": 1,\n\
  \"premise\": \"故事前提\",\n\
  \"ending\": {{\"desired_resolution\": \"终局方向\", \"final_state\": \"终局状态\"}},\n\
  \"protagonist_arc\": \"主角弧线\",\n\
  \"world_imagery\": \"世界观意象\",\n\
  \"main_causal_spine\": \"总主线因果链\"\n\
}}",
        user_message.trim()
    )
}

fn character_patch_prompt(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issues: &ContractIssueList,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
) -> String {
    let profile_hint = genre_patch_prompt_hint(draft, user_message);
    let repairs_existing_authority = !draft.fiction_characters.is_empty();
    let repairs_locked_roles = repairs_existing_authority
        && ContractIssueSet::new(issues).actionable().any(|issue| {
            issue.code == "semantic.user_story_authority"
                && issue.kind == ContractIssueKind::Characters
        });
    let patch_scope_guidance = if repairs_locked_roles {
        "当前角色姓名权威已锁定，但质量门明确指出角色定位与用户故事权威冲突。本轮必须输出完整角色表：所有 canonical_name 必须原样保留且每人恰好出现一次，不得新建、删除、合并或互换姓名；必须按用户权威和稳定故事锚点纠正 role，并同步重写与新 role 一致的欲望、恐惧、底线、弧线和计划登场/离场字段。这是角色功能权威纠错，不是改名。"
    } else if repairs_existing_authority {
        "当前角色权威表已经建立。本轮只输出质量门点名角色的局部修复：canonical_name 必须原样复用；只填写需要替换的欲望、恐惧、底线、弧线、计划登场或计划离场字段，未出错字段可以省略。如果质量门点名计划登场/离场，补丁必须返回对应的 planned_entry/planned_exit，并引用稳定锚点中的实际分卷。不得新建角色或改变角色定位。如果缺口点名权威表外人物姓名，必须在该锚点中删除这个姓名并保留准确的无姓名身份泛称（例如“妹妹”“导师”“对手”），或者改用角色权威表中语义确实对应的 canonical_name；不得原样返回含表外姓名的旧锚点，也不得为绕过质量门而虚构替代姓名。"
    } else {
        "当前尚未建立角色权威表，必须一次生成完整角色表。"
    };
    let character_patch_example = if repairs_locked_roles {
        INITIAL_CHARACTER_PATCH_EXAMPLE
    } else if repairs_existing_authority {
        r#"{"patch_type":"character_patch","characters":[{"canonical_name":"已有姓名","bottom_line":"带具体对象的明确禁令或承诺"}]}"#
    } else {
        INITIAL_CHARACTER_PATCH_EXAMPLE
    };
    let issue_focus = contract_patch_issue_focus_text(&stage_relevant_contract_issues(
        ContractCompletionStage::Characters,
        issues,
    ));
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图补齐阶段：Characters typed patch\n\
用户正在确定小说故事蓝图，当前只需要补齐结构化创作字段。请输出紧凑、可确认、可机读的“故事蓝图字段包”。{STORY_BLUEPRINT_BOUNDARY}你只生成角色字段补丁，不写正文，不解释，不输出 Markdown 代码块。\n\
本阶段只补角色权威表：角色表必须恰好 1 个主角，至少 1 个关系对象/盟友/导师，至少 1 个关键对手/反派/压力源；言情/关系题材的核心情感对象必须明确标为“关键关系对象”，不能降格成普通同伴。\n\
每个 role 只能填写一个与该角色欲望、恐惧、行动职责一致的具体叙事功能；不得把“关系对象/盟友/导师”之类的斜杠选项原样复制进 role，也不得把普通队友、同事或关系对象标成没有实际指导职责的导师。\n\
用户明确使用“女主/女主人公”或“男主/男主人公”时，主角 role 必须保留为“女主”或“男主”，不得泛化成不含该身份信息的“主角”。\n\
用户没有明确指定主角性别时，主角 role 必须写“主角”，不得自行增加“男主”或“女主”权威。\n\
每个关键角色必须有角色、欲望、恐惧、底线、弧线起点、弧线终点、计划登场阶段和计划离场/终局状态；计划登场和计划离场只能引用稳定锚点中实际存在的分卷编号，不能把 expected_chapters 误写成卷数。姓名只用于区分本轮角色槽位，系统会在合同锁定前统一完成本地命名。不要启动正式写作，不要改书名、题材、总字数或终局。\n\n\
{patch_scope_guidance}\n\n\
用户没有指定的角色名和人物锚点由你根据题材、终局和主线补全，不得输出“待补”“待定”“未指定”“暂无”“placeholder”或同义占位。\n\n\
稳定锚点里已有的 canonical_name 和角色定位必须原样复用；只修复当前质量门指出的缺失或无效人物锚点，不要每轮重建角色表。\n\n\
bottom_line 表示角色明确拒绝跨越的边界或必须守住的人、原则、证据、责任，必须写成带具体对象的禁令或承诺；“愿意牺牲别人”“为达目的不择手段”“无论权贵还是平民”这类能力、态度或意愿不是底线。每个欲望、恐惧、底线和弧线字段都必须是语法完整、无缺字、无截断的自然短句。\n\n\
角色锚点中的所有人物姓名必须来自本次 characters 列表；如果欲望、恐惧、底线、弧线字段提到某个人，这个人必须在 characters 中有 canonical_name。不要在字段里临时发明未登记人名。\n\n\
稳定锚点：\n{stable_anchor}\n\n\
当前质量门缺口：{issue_focus}\n\n\
用户最新要求：{}\n\n\
硬性数值：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。\n\
语言边界：{language_boundary}\n\
题材字段提示：{profile_hint}\n\n\
系统会根据 JSON 渲染面板创作草案，也会根据结构化故事蓝图渲染面板创作草案；当前阶段未补齐的字段会由后续阶段继续补齐。\n\
优先输出这个 JSON 补丁；如果本地模型无法稳定 JSON，也必须用同名中文字段逐行输出。只输出一个完整 JSON 对象，字段示例：\n\
{character_patch_example}",
        user_message.trim()
    )
}

fn genre_patch_prompt_hint(draft: &SessionCreationDraftState, user_message: &str) -> String {
    GenrePatchProfile::from_draft(draft, user_message).prompt_hint_text()
}

fn governance_schema_suffix(draft: &SessionCreationDraftState, user_message: &str) -> &'static str {
    GenrePatchProfile::from_draft(draft, user_message).governance_schema_suffix()
}

fn governance_structured_schema_suffix(
    draft: &SessionCreationDraftState,
    user_message: &str,
) -> String {
    let suffix = governance_schema_suffix(draft, user_message).trim();
    if suffix.is_empty() {
        String::new()
    } else {
        suffix
            .strip_prefix(',')
            .map(|value| format!(",\n\t    {value}"))
            .unwrap_or_else(|| format!(",\n\t    {suffix}"))
    }
}

fn plot_patch_prompt(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issues: &ContractIssueList,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
) -> String {
    let profile_hint = genre_patch_prompt_hint(draft, user_message);
    let stage_issues = stage_relevant_contract_issues(ContractCompletionStage::Plot, issues);
    let issue_focus = contract_patch_issue_focus_text(&stage_issues);
    if plot_issue_focus_is_only_near_chapters(&stage_issues) {
        return focused_near_chapters_plot_patch_prompt(
            user_message,
            &issue_focus,
            stable_anchor,
            exact_total_units,
            exact_chapter_unit,
            expected_chapters,
            language_boundary,
            &profile_hint,
        );
    }
    if plot_issue_focus_is_only_payoff_matrix(&stage_issues) {
        return focused_payoff_matrix_plot_patch_prompt(
            user_message,
            &issue_focus,
            stable_anchor,
            exact_total_units,
            exact_chapter_unit,
            expected_chapters,
            language_boundary,
            &profile_hint,
        );
    }
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图补齐阶段：Plot typed patch\n\
用户正在确定小说故事蓝图，当前只需要补齐结构化创作字段。请输出紧凑、可确认、可机读的“故事蓝图字段包”。{STORY_BLUEPRINT_BOUNDARY}你只生成剧情结构字段补丁，不写正文，不解释，不输出 Markdown 代码块。\n\
本阶段只补分卷/阶段、近期章节目标、不可逆变化和伏笔/兑现矩阵。不要改书名、角色名、题材、字数或治理字段。\n\n\
如果当前质量门缺口指向大纲本体中的残句、角色定位冲突、角色与自身形成关系、权威表外角色或其他文本污染，补丁必须提供完整修正后的 `raw_outline`；不能只返回 volumes、near_chapters 或 payoff_matrix 而保留旧大纲。\n\n\
如果质量门指出角色权威名被用作公司、组织、地点、机构、协议、系统或其他非人物实体，必须保留该 canonical_name 作为人物姓名，并为冲突的非人物实体改用一个与全部角色姓名不同的故事内名称；在 raw_outline、volumes 和 near_chapters 中同步替换，不能删除该人物、改变角色名或把同一个名字继续同时用于人物和实体。\n\n\
如果质量门点名用户明确禁用的名字，必须从 raw_outline、volumes 和 near_chapters 中同步删除；该名字不属于角色权威表时，改用准确的职责或身份泛称，不得新造未登记角色名。\n\n\
用户没有指定的分卷、章节目标和伏笔由你根据题材、终局、主线和角色弧线补全，不得输出“待补”“待定”“未指定”“暂无”“placeholder”或同义占位。\n\n\
必须输出 1 到 5 个 volumes，以及连续编号、从第 1 章开始的 3 到 8 个 near_chapters。near_chapters 只覆盖全书开篇窗口：当 expected_chapters 大于近期章节末章时，不得在近期章节内完成权威终局、主冲突总解决、主角弧线终点或终局后的稳定状态；最后一个近期章节必须仍给后续分卷留下可推进的主线债务。有多卷时，只有最后一卷可以完成权威终局、主角弧线终点和终局稳定状态；非末卷必须留有明确未解决的主线债务，不得把终局提前完成后再用后续卷回顾或铺续作。每卷都要有具体 objective 和不可逆 ending_change；每章都要有具体事件 goal。每个 expected_turn 只写一个点名受影响人物或权威实体、能由章末一段连续正文证明的完整结果，不能只写数字、章节号、抽象主题或总结性空话，也不得串联互不依赖的多个变化。raw_outline 只写故事规划，不得写入总字数、每章字数、预计章数、“全书共多少章”或确认/写作流程说明。角色引用只能使用稳定锚点中的权威姓名。分卷和章节必须服从稳定锚点中的故事前提、世界规则、主线因果与终局，不能凭空扩大人物、工具或制度的能力边界，也不能依赖未建立的关键能力完成阶段目标。\n\n\
如果当前缺口包含 `outline.longform_position`，必须完整改写 volumes 的阶段边界，不能原样返回当前分卷：若倒数第二卷已经完成权威终局，而最后一卷只是终局后的尾声、回顾或稳定生活，就合并/删除单独尾声卷，或让倒数第二卷保留未解决的主线债务并把权威终局移到实际最后一卷。最终输出中，只有 volumes 数组的最后一个元素可以出现权威终局、主冲突总解决或主角弧线终点。\n\n\
如果当前缺口包含 `outline.terminal_coverage`，必须让 volumes 最后一个元素的 objective 或 ending_change 在语义上完整执行稳定锚点中“终局方向”的全部核心行动，并写出对应直接结果和不可逆变化；不能把其中的关键人物、物件、机制、行动或结果改成较弱的泛化结尾，也不要为了字面相同而把完整终局复制到职责不符的字段。保持当前分卷数量和不涉及终局的阶段边界；但如果任一非末卷已经用原句或同义表达完成了终局方向中的选择、核心行动、关系确认、主冲突总解决或不可逆结果，必须把该非末卷改成尚未完成终局的准备、代价或未解决债务，并把完整终局移到末卷。这种迁移属于修复当前 terminal_coverage，不得因“保持非末卷阶段边界”而保留提前完成的终局，也不得改动无关分卷或既有角色登场/离场锚点。\n\n\
如果当前缺口包含 `semantic.outline_character_authority`，并且点名卷目标、卷尾变化、兑现矩阵、终局状态、逻辑顺序或“直接跳到终局”，必须把它当作同一个 Plot 因果链问题处理：保留角色权威、世界规则和终局方向不变；同步重写被点名 volume 的 objective 与 ending_change，使卷目标先建立达成终局所需的条件、代价或突破，卷尾变化只写该阶段真实完成的不可逆结果；如果该卷是末卷，objective 必须包含通向终局核心行动的因果步骤，ending_change 才能完成终局结果；如果该卷不是末卷，不得出现终局完成或终局稳定状态。payoff_matrix 中镜像同一错误终局跳跃的 payoff_target 也必须一并改写为同一阶段的伏笔兑现或末卷终局兑现，不能只改 volumes 留下旧兑现矩阵继续触发相同语义冲突。\n\n\
稳定锚点：\n{stable_anchor}\n\n\
当前质量门缺口：{issue_focus}\n\n\
用户最新要求：{}\n\n\
硬性数值：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。\n\
语言边界：{language_boundary}\n\
题材字段提示：{profile_hint}\n\n\
系统会根据 JSON 渲染面板创作草案，也会根据结构化故事蓝图渲染面板创作草案；当前阶段未补齐的字段会由后续阶段继续补齐。\n\
优先输出这个 JSON 补丁；如果本地模型无法稳定 JSON，也必须用同名中文字段逐行输出。只输出一个完整 JSON 对象，字段示例：\n\
{{\n\
  \"patch_type\": \"plot_patch\",\n\
  \"outline\": {{\n\
    \"volumes\": [{{\"title\":\"卷名\",\"objective\":\"本卷必须达成的具体阶段目标\",\"ending_change\":\"卷尾发生的不可逆事件变化\"}}],\n\
    \"near_chapters\": [{{\"number\":1,\"goal\":\"本章具体事件目标\",\"expected_turn\":\"本章结束时发生的不可逆事件变化\"}},{{\"number\":2,\"goal\":\"本章具体事件目标\",\"expected_turn\":\"本章结束时发生的不可逆事件变化\"}},{{\"number\":3,\"goal\":\"本章具体事件目标\",\"expected_turn\":\"本章结束时发生的不可逆事件变化\"}}],\n\
    \"raw_outline\":\"全书大纲摘要\"\n\
  }},\n\
  \"payoff_matrix\": [{{\"promise\":\"承诺/伏笔\",\"payoff_target\":\"兑现方式\",\"status\":\"planned\"}}]\n\
}}",
        user_message.trim()
    )
}

fn focused_near_chapters_plot_patch_prompt(
    user_message: &str,
    issue_focus: &str,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
    profile_hint: &str,
) -> String {
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图补齐阶段：Plot typed patch / near_chapters focused\n\
当前分卷、终局、角色权威和其他故事字段已经稳定。本轮只完整替换 `outline.near_chapters`，不要输出 volumes、raw_outline、payoff_matrix、书名、角色、题材、字数或治理字段；不要写正文、解释或 Markdown 代码块。\n\
必须输出 3 到 8 个近期章节，并严格从 number=1 开始连续递增，不能缺号、跳号、重号或从第2章开始。每章都必须同时有具体事件 `goal` 和事件变化式 `expected_turn`；不能只写章节号、主题、总结或数字占位。近期章节只是全书开篇窗口，最后一章仍要保留后续主线债务，不得提前完成权威终局、主冲突总解决或主角弧线终点。\n\
如果当前质量门缺口包含用户对具体章节的明确修订，必须把修订指定的人物、身份、事件先后和因果事实实际写入对应章节的 `goal` 或 `expected_turn`，并替换与之冲突的旧内容；不能只保留大意、只写无姓名角色、把明确身份降级为泛称，或原样返回未落实修订的旧章节。\n\
所有角色名只能使用稳定锚点中的 canonical_name；章节事件必须服从既有分卷、故事前提、世界规则、主线因果和终局，不得重写这些稳定字段。\n\n\
稳定锚点：\n{stable_anchor}\n\n\
当前质量门缺口：{issue_focus}\n\n\
用户最新要求：{}\n\n\
硬性数值：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。\n\
语言边界：{language_boundary}\n\
题材字段提示：{profile_hint}\n\n\
只输出一个完整 JSON 对象：\n\
{{\"patch_type\":\"plot_patch\",\"outline\":{{\"near_chapters\":[{{\"number\":1,\"goal\":\"第1章具体事件目标\",\"expected_turn\":\"第1章末不可逆事件变化\"}},{{\"number\":2,\"goal\":\"第2章具体事件目标\",\"expected_turn\":\"第2章末不可逆事件变化\"}},{{\"number\":3,\"goal\":\"第3章具体事件目标\",\"expected_turn\":\"第3章末不可逆事件变化\"}}]}}}}",
        user_message.trim()
    )
}

#[allow(clippy::too_many_arguments)]
fn focused_payoff_matrix_plot_patch_prompt(
    user_message: &str,
    issue_focus: &str,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
    profile_hint: &str,
) -> String {
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图补齐阶段：Plot typed patch / payoff_matrix focused\n\
当前书名、角色、世界规则、分卷和近期章节已经稳定。本轮只完整替换 `payoff_matrix`，不得输出或改写任何其他字段；不要写正文、解释或 Markdown 代码块。\n\
每一项必须同时包含非空的 `promise`、具体的 `payoff_target` 和 `status=planned`。promise 必须写清开篇或前期建立的具体承诺、线索或异常；payoff_target 必须写清后续由权威角色执行的具体行动及可观察结果。不得使用“阶段证据”“主线债务”“权威终局”“完成兑现”等规划占位语。\n\
所有角色名只能使用稳定锚点中的 canonical_name；伏笔及兑现必须服从既有故事前提、世界规则、主线因果、终局与分卷边界。\n\n\
稳定锚点：\n{stable_anchor}\n\n\
当前质量门缺口：{issue_focus}\n\n\
用户最新要求：{}\n\n\
硬性数值：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。\n\
语言边界：{language_boundary}\n\
题材字段提示：{profile_hint}\n\n\
只输出一个完整 JSON 对象：\n\
{{\"patch_type\":\"plot_patch\",\"payoff_matrix\":[{{\"promise\":\"前期建立的具体承诺或伏笔\",\"payoff_target\":\"后续具体行动及可观察结果\",\"status\":\"planned\"}}]}}",
        user_message.trim()
    )
}

fn governance_patch_prompt(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issues: &ContractIssueList,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
) -> String {
    let profile_hint = genre_patch_prompt_hint(draft, user_message);
    let stage_issues = stage_relevant_contract_issues(ContractCompletionStage::Governance, issues);
    let issue_focus = contract_patch_issue_focus_text(&stage_issues);
    if governance_issue_focus_is_only_visible_fields(&stage_issues) {
        return focused_visible_governance_patch_prompt(
            draft,
            user_message,
            &stage_issues,
            &issue_focus,
            stable_anchor,
            exact_total_units,
            exact_chapter_unit,
            expected_chapters,
            language_boundary,
            &profile_hint,
        );
    }
    if governance_issue_focus_is_only_relationship_ledger(&stage_issues) {
        return focused_relationship_ledger_governance_patch_prompt(
            draft,
            user_message,
            &issue_focus,
            stable_anchor,
            exact_total_units,
            exact_chapter_unit,
            expected_chapters,
            language_boundary,
        );
    }
    let genre_schema = governance_structured_schema_suffix(draft, user_message);
    let (primary_name, related_name, antagonist_name) = governance_schema_authority_names(draft);
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图补齐阶段：Governance typed patch\n\
用户正在确定小说故事蓝图，当前只需要补齐结构化创作字段。请输出紧凑、可确认、可机读的“故事蓝图字段包”。{STORY_BLUEPRINT_BOUNDARY}你只生成治理字段补丁，不写正文，不解释，不输出 Markdown 代码块。\n\
	本阶段只补主题、世界规则、叙事风格、必须避免、情感承诺、节奏缓冲、关系线、反派/压力源、叙事视角，以及小说审美/节奏控制字段。不要改书名、角色名、题材、字数、伏笔兑现矩阵或章节正文。\n\n\
用户没有指定的治理字段由你根据题材、终局、主线和角色弧线补全，不得输出“待补”“待定”“未指定”“暂无”“placeholder”或同义占位。\n\
本阶段必须输出 world_rules，且至少 3 条。world_rules 必须是故事世界如何运行的可执行规则：包含能力/资源/制度的代价、限制、失败后果或稀缺条件；不能复述世界观意象，不能写成“不要怎样写”的写作禁令，也不能留空。\n\
如果题材有成长体系、资源经济、社会秩序或地理/时间约束，也必须把这些字段填成具体约束；不要只写体系名或地点名。\n\n\
稳定锚点：\n{stable_anchor}\n\n\
当前质量门缺口：{issue_focus}\n\n\
用户最新要求：{}\n\n\
硬性数值：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。\n\
语言边界：{language_boundary}\n\
题材字段提示：{profile_hint}\n\n\
系统会根据 JSON 渲染面板创作草案，也会根据结构化故事蓝图渲染面板创作草案；当前阶段未补齐的字段会由后续阶段继续补齐。\n\
优先输出这个 JSON 补丁；如果本地模型无法稳定 JSON，也必须用同名中文字段逐行输出。只输出一个完整 JSON 对象，字段示例：\n\
{{\n\
  \"patch_type\": \"governance_patch\",\n\
  \"themes\": [\"主题\"],\n\
  \"world_rules\": [\"世界规则\"],\n\
  \"style_rules\": [\"叙事风格\"],\n\
  \"must_avoid\": [\"必须避免\"],\n\
  \"emotional_contract\": {{\"primary_emotion\":\"主情绪\",\"emotional_promise\":\"情感承诺\",\"emotional_beats\":[\"情绪阶段\"],\"relief_beats\":[\"适合题材的轻松/反差/幽默缓冲\"],\"ending_emotional_state\":\"结尾情感状态\"}},\n\
	  \"relationship_ledger\": [{{\"characters\":[{primary_name},{related_name}],\"relationship_type\":\"关系类型\",\"start_state\":\"起始关系\",\"desired_end_state\":\"终局关系\",\"conflicts\":[\"冲突\"]}}],\n\
	  \"antagonist_pressure\": {{\"primary_pressure\":\"主要对手/压力源及其持续压力\",\"antagonists\":[{{\"name\":{antagonist_name},\"goal\":\"目标\",\"resources\":[\"资源\"],\"current_move\":\"当前行动\",\"defeat_condition\":\"失败条件\"}}]}},\n\
	  \"narration_contract\": {{\"pov\":\"叙事视角\",\"narrative_distance\":\"叙事距离\",\"dialogue_style\":\"对白风格\"}},\n\
	  \"structured\": {{\n\
	    \"scene_type_mix\": {{\"action\":\"动作/冲突戏比例或使用条件\",\"dialogue\":\"对话戏比例或使用条件\",\"everyday\":\"日常/缓冲戏比例或使用条件\",\"reveal\":\"信息揭示戏比例或使用条件\",\"emotional\":\"情感戏比例或使用条件\",\"turning_point\":\"转折戏比例或使用条件\",\"balance_rule\":\"场景类型轮换规则\"}},\n\
	    \"character_voice_ledger\": [{{\"character\":{primary_name},\"voice_style\":\"说话方式\",\"catchphrases\":[\"可偶尔出现的口头禅\"],\"forbidden_expressions\":[\"不该说的话\"],\"dialogue_rules\":[\"对白规则\"]}}],\n\
	    \"reader_promise\": {{\"core_hook\":\"读者为什么继续读\",\"pleasure_points\":[\"爽点/期待点\"],\"curiosity_engine\":\"持续好奇机制\",\"payoff_style\":\"兑现方式\"}},\n\
	    \"chapter_ending_rotation\": {{\"planned_rotation\":[\"悬念\",\"情绪落点\",\"反转\",\"阶段收束\"],\"avoid_repetition_rule\":\"章尾形态避免连续重复的规则\"}},\n\
	    \"conflict_pressure_curve\": {{\"global_curve\":[{{\"range\":\"章节/卷范围\",\"pressure_level\":\"升压/缓冲/爆发/回落\",\"function\":\"剧情功能\"}}],\"release_strategy\":\"降压和缓冲策略\",\"peak_policy\":\"爆发点安排\"}},\n\
	    \"motif_ledger\": [{{\"motif\":\"反复出现的意象/动作/地点\",\"meaning\":\"当前含义\",\"evolution\":[\"阶段变化\"],\"payoff_target\":\"最终如何变化或兑现\"}}],\n\
	    \"reveal_schedule\": [{{\"secret\":\"秘密/信息\",\"reader_knows\":\"读者知道到什么程度\",\"protagonist_knows\":\"主角知道到什么程度\",\"antagonist_knows\":\"对手知道到什么程度\",\"reveal_window\":\"揭示窗口\",\"status\":\"planned\"}}],\n\
	    \"relationship_interaction_quotas\": [{{\"relationship\":\"关系线\",\"characters\":[{primary_name},{related_name}],\"cadence\":\"互动频率/不能断线多久\",\"next_due\":\"下一次推进窗口\",\"required_interaction\":\"必须出现的互动类型\"}}]{genre_schema}\n\
	  }}\n\
	}}",
        user_message.trim()
    )
}

fn governance_schema_authority_names(
    draft: &SessionCreationDraftState,
) -> (String, String, String) {
    let characters = draft
        .fiction_characters
        .iter()
        .map(|line| super::draft_character_line_to_contract(line))
        .filter(|character| !value_missing(&character.canonical_name))
        .collect::<Vec<_>>();
    let primary = characters
        .iter()
        .find(|character| character.role_looks_primary())
        .or_else(|| characters.first());
    let related = characters
        .iter()
        .find(|character| {
            !character.role_looks_primary()
                && !character.role.contains("对手")
                && !character.role.contains("反派")
                && !character.role.contains("压力源")
        })
        .or_else(|| {
            characters.iter().find(|character| {
                primary.is_none_or(|primary| character.canonical_name != primary.canonical_name)
            })
        });
    let antagonist = characters
        .iter()
        .find(|character| {
            character.role.contains("对手")
                || character.role.contains("反派")
                || character.role.contains("压力源")
        })
        .or(related);
    let json_name =
        |character: Option<&super::super::creation_contract_model::CharacterContract>| {
            serde_json::to_string(
                character
                    .map(|character| character.canonical_name.trim())
                    .unwrap_or_default(),
            )
            .unwrap_or_else(|_| "\"\"".to_string())
        };
    (
        json_name(primary),
        json_name(related),
        json_name(antagonist),
    )
}

fn contract_patch_issue_focus_text(issues: &[String]) -> String {
    let mut items = issues
        .iter()
        .map(|issue| issue.trim())
        .filter(|issue| !issue.is_empty())
        .take(8)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    items.sort();
    items.dedup();
    if items.is_empty() {
        "补齐当前阶段仍缺失的结构化字段；不要重复已稳定字段。".to_string()
    } else {
        format!(
            "{}。只修复这些缺口；质量门明确点名的字段视为不稳定并必须替换，其他稳定锚点不得改写。",
            items.join("；")
        )
    }
}

pub(super) fn stage_relevant_contract_issues(
    stage: ContractCompletionStage,
    issues: &ContractIssueList,
) -> Vec<String> {
    let issue_set = ContractIssueSet::new(issues);
    let mut filtered = issue_set
        .iter()
        .filter(|issue| contract_issue_matches_completion_stage(stage, issue))
        .map(|issue| issue.text.clone())
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        filtered = issue_set
            .iter()
            .filter(|issue| {
                (issue.kind.is_diagnostic()
                    && contract_issue_matches_completion_stage(stage, issue))
                    || matches!(issue.kind, ContractIssueKind::Other)
            })
            .map(|issue| issue.text.clone())
            .collect();
    }
    filtered.sort();
    filtered.dedup();
    filtered
}

fn contract_issue_matches_completion_stage(
    stage: ContractCompletionStage,
    issue: ClassifiedContractIssue<'_>,
) -> bool {
    if let Some(feedback_stage) = issue.code.strip_prefix("contract.patch_feedback.") {
        return feedback_stage == contract_completion_stage_key(stage);
    }
    if issue.kind.is_diagnostic() {
        return true;
    }
    if issue.code == "semantic.user_story_authority" {
        return matches!(
            (stage, issue.kind),
            (
                ContractCompletionStage::Skeleton,
                ContractIssueKind::Skeleton
            ) | (
                ContractCompletionStage::Characters,
                ContractIssueKind::Characters
            ) | (ContractCompletionStage::Plot, ContractIssueKind::Plot)
        );
    }
    match stage {
        ContractCompletionStage::Skeleton => {
            matches!(issue.kind, ContractIssueKind::Skeleton)
                || issue.code.starts_with("contract.title")
        }
        ContractCompletionStage::Characters => {
            matches!(issue.kind, ContractIssueKind::Characters)
        }
        ContractCompletionStage::Plot => {
            matches!(issue.kind, ContractIssueKind::Plot)
                && !issue.code.starts_with("contract.title")
        }
        ContractCompletionStage::Governance => {
            matches!(issue.kind, ContractIssueKind::Governance)
        }
    }
}

pub(super) const fn contract_completion_stage_key(stage: ContractCompletionStage) -> &'static str {
    match stage {
        ContractCompletionStage::Skeleton => "skeleton",
        ContractCompletionStage::Characters => "characters",
        ContractCompletionStage::Plot => "plot",
        ContractCompletionStage::Governance => "governance",
    }
}

pub(super) fn governance_issue_focus_is_only_visible_fields(issues: &[String]) -> bool {
    let actionable = issues
        .iter()
        .map(|issue| issue.trim())
        .filter(|issue| !issue.is_empty())
        .filter(|issue| {
            let lowered = issue.to_ascii_lowercase();
            !lowered.contains("typed patch")
                && !lowered.contains("governance_patch")
                && !issue.contains("作用域校验")
                && !creation_contract_issue_is_title_metadata(issue)
        })
        .collect::<Vec<_>>();
    !actionable.is_empty()
        && actionable
            .iter()
            .all(|issue| visible_governance_issue_field(issue).is_some())
}

pub(super) fn plot_issue_focus_is_only_near_chapters(issues: &[String]) -> bool {
    let actionable = issues
        .iter()
        .map(|issue| issue.trim())
        .filter(|issue| !issue.is_empty())
        .filter(|issue| {
            let lowered = issue.to_ascii_lowercase();
            (!lowered.contains("typed patch") || issue.contains("contract.explicit_revision"))
                && !lowered.contains("plot_patch")
                && !issue.contains("作用域校验")
        })
        .collect::<Vec<_>>();
    !actionable.is_empty()
        && actionable.iter().all(|issue| {
            let lowered = issue.to_ascii_lowercase();
            issue.contains("近期章节") || lowered.contains("near_chapter")
        })
}

pub(super) fn plot_issue_focus_is_only_payoff_matrix(issues: &[String]) -> bool {
    let actionable = issues
        .iter()
        .map(|issue| issue.trim())
        .filter(|issue| !issue.is_empty())
        .filter(|issue| {
            let lowered = issue.to_ascii_lowercase();
            (!lowered.contains("typed patch") || issue.contains("contract.explicit_revision"))
                && !lowered.contains("plot_patch")
                && !issue.contains("作用域校验")
        })
        .collect::<Vec<_>>();
    !actionable.is_empty()
        && actionable.iter().all(|issue| {
            let lowered = issue.to_ascii_lowercase();
            issue.contains("兑现矩阵")
                || issue.contains("伏笔/承诺")
                || lowered.contains("payoff_matrix")
        })
}

fn visible_governance_issue_field(issue: &str) -> Option<&'static str> {
    let lowered = issue.to_ascii_lowercase();
    if issue.contains("世界规则") || lowered.contains("world_rules") {
        Some("world_rules")
    } else if issue.contains("核心主题") || lowered.contains("themes") || lowered.contains("theme")
    {
        Some("themes")
    } else if issue.contains("必须避免")
        || issue.contains("写作禁区")
        || lowered.contains("must_avoid")
    {
        Some("must_avoid")
    } else if issue.contains("叙事风格")
        || issue.contains("文风")
        || lowered.contains("style_rules")
    {
        Some("style_rules")
    } else {
        None
    }
}

pub(super) fn governance_issue_focus_is_only_relationship_ledger(issues: &[String]) -> bool {
    let actionable = issues
        .iter()
        .map(|issue| issue.trim())
        .filter(|issue| !issue.is_empty())
        .filter(|issue| {
            let lowered = issue.to_ascii_lowercase();
            !lowered.contains("typed patch")
                && !lowered.contains("governance_patch")
                && !issue.contains("作用域校验")
                && !creation_contract_issue_is_title_metadata(issue)
        })
        .collect::<Vec<_>>();
    !actionable.is_empty()
        && actionable.iter().all(|issue| {
            let lowered = issue.to_ascii_lowercase();
            issue.contains("关系账本")
                || issue.contains("关系线")
                || lowered.contains("relationship_ledger")
                || lowered.contains("relationship")
        })
}

#[allow(clippy::too_many_arguments)]
fn focused_relationship_ledger_governance_patch_prompt(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issue_focus: &str,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
) -> String {
    let (primary_name, related_name, antagonist_name) = governance_schema_authority_names(draft);
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图补齐阶段：Governance typed patch / relationship_ledger focused\n\
用户正在确定小说故事蓝图，当前只需要修复关系账本。请输出紧凑、可确认、可机读的“故事蓝图字段包”。{STORY_BLUEPRINT_BOUNDARY}你只生成治理字段补丁，不写正文，不解释，不输出 Markdown 代码块。\n\
本轮只输出完整替换后的 relationship_ledger。不要改书名、角色权威表、角色名、题材、字数、世界观、终局、章节规划或正文。\n\
每条关系必须恰好包含两个不同的已锁定 canonical_name；不得使用群体、组织、职业、身份标签、泛称、旧名或角色权威表之外的名字，也不得新建角色。关系类型、起始状态、终局状态和冲突必须来自当前故事。\n\
请覆盖故事需要持续追踪的关键人物关系；质量门点名的旧关系账本视为无效，不得照抄其中的非法参与者。\n\n\
稳定锚点：\n{stable_anchor}\n\n\
当前质量门缺口：{issue_focus}\n\n\
用户最新要求：{}\n\n\
硬性数值：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。\n\
语言边界：{language_boundary}\n\n\
只输出一个完整 JSON 对象，格式为：\n\
{{\n\
  \"patch_type\": \"governance_patch\",\n\
  \"relationship_ledger\": [\n\
    {{\"characters\":[{primary_name},{related_name}],\"relationship_type\":\"具体关系类型\",\"start_state\":\"故事开始时的关系状态\",\"desired_end_state\":\"终局关系状态\",\"conflicts\":[\"推动关系变化的具体冲突\"]}},\n\
    {{\"characters\":[{primary_name},{antagonist_name}],\"relationship_type\":\"具体关系类型\",\"start_state\":\"故事开始时的关系状态\",\"desired_end_state\":\"终局关系状态\",\"conflicts\":[\"推动关系变化的具体冲突\"]}}\n\
  ]\n\
}}",
        user_message.trim()
    )
}

#[allow(clippy::too_many_arguments)]
fn focused_visible_governance_patch_prompt(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issues: &[String],
    issue_focus: &str,
    stable_anchor: &str,
    exact_total_units: &str,
    exact_chapter_unit: &str,
    expected_chapters: usize,
    language_boundary: &str,
    profile_hint: &str,
) -> String {
    let _ = draft;
    let requested_fields = issues
        .iter()
        .filter_map(|issue| visible_governance_issue_field(issue))
        .collect::<std::collections::BTreeSet<_>>();
    let mut schema_fields = Vec::new();
    let mut field_requirements = Vec::new();
    if requested_fields.contains("themes") {
        schema_fields.push("  \"themes\": [\"贯穿主线并能由人物选择兑现的核心主题\"]");
        field_requirements.push("themes 至少 1 条，必须能由主角选择、代价和终局兑现");
    }
    if requested_fields.contains("world_rules") {
        schema_fields.push(
            "  \"world_rules\": [\"规则1：能力或资源的代价\", \"规则2：制度限制或失败后果\", \"规则3：推动关系线或冲突线的稀缺条件\"]",
        );
        field_requirements
            .push("world_rules 至少 3 条，每条必须包含代价、限制、失败后果或稀缺条件");
    }
    if requested_fields.contains("style_rules") {
        schema_fields.push("  \"style_rules\": [\"可执行的叙事视角、节奏或语言规则\"]");
        field_requirements.push("style_rules 至少 1 条，必须是可执行的叙事规则");
    }
    if requested_fields.contains("must_avoid") {
        schema_fields
            .push("  \"must_avoid\": [\"会破坏当前故事因果、人物弧线或题材承诺的明确禁区\"]");
        field_requirements.push("must_avoid 至少 1 条，必须针对当前故事的漂移风险");
    }
    let schema_fields = schema_fields.join(",\n");
    let field_requirements = field_requirements.join("；");
    format!(
        "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
故事蓝图补齐阶段：Governance typed patch / visible fields focused\n\
用户正在确定小说故事蓝图，当前只需要补齐结构化创作字段。请输出紧凑、可确认、可机读的“故事蓝图字段包”。{STORY_BLUEPRINT_BOUNDARY}你只生成治理字段补丁，不写正文，不解释，不输出 Markdown 代码块。\n\
本轮只补质量门点名的可见治理字段。不要输出关系账本、伏笔矩阵、反派压力或其他已经稳定的结构化字段；不要改书名、角色名、题材、字数、终局、章节规划或正文。\n\
字段要求：{field_requirements}。world_rules 不能复述世界观意象，也不能写成写作禁令；must_avoid 不能冒充世界运行规则。\n\
用户没有指定的字段由你根据题材、终局、主线和角色弧线补全，不得输出“待补”“待定”“未指定”“暂无”“placeholder”或同义占位。\n\n\
稳定锚点：\n{stable_anchor}\n\n\
当前质量门缺口：{issue_focus}\n\n\
用户最新要求：{}\n\n\
硬性数值：target_units={exact_total_units}; chapter_unit_target={exact_chapter_unit}; expected_chapters={expected_chapters}。\n\
语言边界：{language_boundary}\n\
题材字段提示：{profile_hint}\n\n\
优先输出这个 JSON 补丁；如果本地模型无法稳定 JSON，也必须用同名中文字段逐行输出。只输出一个完整 JSON 对象，字段示例：\n\
{{\n\
  \"patch_type\": \"governance_patch\",\n\
{schema_fields}\n\
}}",
        user_message.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_story_semantic_issue_only_routes_to_its_typed_owner() {
        let skeleton_issues = ContractIssueList::single(
            "semantic.user_story_authority",
            ContractIssueKind::Skeleton,
            "user_authority",
            "ContractBlocker[semantic.user_story_authority]: 故事字段与用户权威冲突",
        );

        assert!(!stage_relevant_contract_issues(
            ContractCompletionStage::Skeleton,
            &skeleton_issues
        )
        .is_empty());
        assert!(
            stage_relevant_contract_issues(ContractCompletionStage::Plot, &skeleton_issues)
                .is_empty()
        );
        let plot_issues = ContractIssueList::single(
            "semantic.user_story_authority",
            ContractIssueKind::Plot,
            "outline",
            "ContractBlocker[semantic.user_story_authority]: 第2卷与用户修订冲突",
        );
        assert!(
            !stage_relevant_contract_issues(ContractCompletionStage::Plot, &plot_issues).is_empty()
        );
        assert!(
            stage_relevant_contract_issues(ContractCompletionStage::Skeleton, &plot_issues)
                .is_empty()
        );

        let character_issues = ContractIssueList::single(
            "semantic.user_story_authority",
            ContractIssueKind::Characters,
            "角色权威表",
            "ContractBlocker[semantic.user_story_authority]: 男主被标成对手",
        );
        assert!(!stage_relevant_contract_issues(
            ContractCompletionStage::Characters,
            &character_issues,
        )
        .is_empty());
        assert!(stage_relevant_contract_issues(
            ContractCompletionStage::Skeleton,
            &character_issues,
        )
        .is_empty());
    }

    fn typed_issues(
        kind: ContractIssueKind,
        messages: impl IntoIterator<Item = String>,
    ) -> ContractIssueList {
        ContractIssueList::from_messages("test.contract_issue", kind, "test", messages)
    }

    #[test]
    fn initial_character_patch_example_keeps_three_distinct_slots_after_normalization() {
        let draft = super::build_initial_creation_draft(
            "session-character-example-slots",
            "fiction",
            "写一部生态悬疑小说，每章2500字，一共10万字。",
        )
        .expect("draft");

        let patch =
            normalize_creation_contract_patch_boundary(&draft, INITIAL_CHARACTER_PATCH_EXAMPLE)
                .expect("character patch example");
        let CreationContractPatch::Characters(patch) = patch else {
            panic!("expected character patch");
        };
        let names = patch
            .characters
            .iter()
            .map(|character| character.canonical_name.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(patch.characters.len(), 3);
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn locked_character_repair_prompt_requires_external_names_to_be_generalized() {
        let mut draft = super::build_initial_creation_draft(
            "session-character-external-name-repair",
            "fiction",
            "写一部都市重生小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "姓名：季望真；角色：主角；欲望：找回妹妹林瑶；恐惧：被彻底遗忘；底线：不牺牲盟友；弧线起点：被动接受；弧线终点：主动重构".to_string(),
            "姓名：商云野；角色：关键关系对象；欲望：建立独立记忆库；恐惧：数据被篡改；底线：守住原始代码；弧线起点：多疑疏离；弧线终点：信任交付".to_string(),
            "姓名：阮星安；角色：对手；欲望：垄断全城记忆；恐惧：失去控制；底线：不放弃核心服务器；弧线起点：傲慢掌控；弧线终点：失控崩塌".to_string(),
        ];
        let issues = ContractIssueList::single(
            "contract.character_reference",
            ContractIssueKind::Characters,
            "characters",
            "ContractBlocker: 角色 `季望真` 的欲望锚点引用了权威表外角色 `林瑶`",
        );

        let prompt = character_patch_prompt(
            &draft,
            "继续修复当前合同",
            &issues,
            "稳定锚点",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(prompt.contains("保留准确的无姓名身份泛称"), "{prompt}");
        assert!(
            prompt.contains("不得原样返回含表外姓名的旧锚点"),
            "{prompt}"
        );
        assert!(
            prompt.contains("不得为绕过质量门而虚构替代姓名"),
            "{prompt}"
        );
    }

    #[test]
    fn governance_world_rules_focus_ignores_title_metadata_diagnostics() {
        let issues = vec![
            "ContractBlocker: 小说合同缺少世界规则".to_string(),
            "ContractBlocker: 琉璃契: 书名命名理由没有解释标题中的关键字".to_string(),
        ];

        assert!(governance_issue_focus_is_only_visible_fields(&issues));
    }

    #[test]
    fn governance_world_rules_focus_rejects_mixed_foundational_issues() {
        let issues = vec![
            "ContractBlocker: 小说合同缺少世界规则".to_string(),
            "ContractBlocker: 小说合同缺少角色权威表".to_string(),
        ];

        assert!(!governance_issue_focus_is_only_visible_fields(&issues));
    }

    #[test]
    fn governance_visible_focus_accepts_multiple_basic_governance_fields() {
        let issues = vec![
            "ContractBlocker: 小说合同缺少世界规则".to_string(),
            "ContractBlocker: 小说合同缺少必须避免".to_string(),
            "ContractBlocker: 小说合同缺少核心主题".to_string(),
        ];
        assert!(governance_issue_focus_is_only_visible_fields(&issues));
    }

    #[test]
    fn focused_visible_governance_prompt_only_requests_missing_fields() {
        let draft = super::build_initial_creation_draft(
            "visible-governance-focused-prompt",
            "fiction",
            "写一部玄幻小说，每章5000字，一共100万字。",
        )
        .expect("draft");
        let issues = vec![
            "ContractBlocker: 小说合同缺少世界规则".to_string(),
            "ContractBlocker: 小说合同缺少必须避免".to_string(),
            "ContractBlocker: 小说合同缺少核心主题".to_string(),
        ];
        let issues = typed_issues(ContractIssueKind::Governance, issues);

        let prompt = governance_patch_prompt(
            &draft,
            "其他内容你来决定",
            &issues,
            "稳定锚点",
            "1000000",
            "5000",
            200,
            "使用中文",
        );

        assert!(prompt.contains("visible fields focused"), "{prompt}");
        assert!(prompt.contains("\"themes\""), "{prompt}");
        assert!(prompt.contains("\"world_rules\""), "{prompt}");
        assert!(prompt.contains("\"must_avoid\""), "{prompt}");
        assert!(!prompt.contains("\"relationship_ledger\""), "{prompt}");
        assert!(!prompt.contains("\"payoff_matrix\""), "{prompt}");
    }

    #[test]
    fn focused_payoff_prompt_cannot_rebuild_other_contract_authority() {
        let draft = super::build_initial_creation_draft(
            "payoff-focused-prompt",
            "fiction",
            "写一部校园悬疑小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        let issues = typed_issues(
            ContractIssueKind::Plot,
            vec![
                "ContractBlocker: 小说合同兑现矩阵第1项缺少具体承诺或伏笔".to_string(),
                "ContractBlocker: 小说合同兑现矩阵第2项缺少生命周期状态".to_string(),
            ],
        );

        let prompt = plot_patch_prompt(
            &draft,
            "继续修复当前合同",
            &issues,
            "稳定锚点",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(prompt.contains("payoff_matrix focused"), "{prompt}");
        assert!(prompt.contains("\"payoff_matrix\""), "{prompt}");
        assert!(!prompt.contains("\"volumes\""), "{prompt}");
        assert!(!prompt.contains("\"near_chapters\""), "{prompt}");
        assert!(!prompt.contains("\"world_rules\""), "{prompt}");
        assert!(!prompt.contains("\"canonical_title\""), "{prompt}");
    }

    #[test]
    fn governance_relationship_focus_recognizes_authority_only_issues() {
        let issues = vec![
            "ContractBlocker: 关系线角色 `工友群体` 不在角色权威表中".to_string(),
            "ContractBlocker: 关系账本[2]引用角色权威表之外的角色 `工友群体`".to_string(),
        ];

        assert!(governance_issue_focus_is_only_relationship_ledger(&issues));
    }

    #[test]
    fn focused_relationship_prompt_uses_locked_character_authority() {
        let mut draft = super::build_initial_creation_draft(
            "relationship-ledger-focused-prompt",
            "fiction",
            "写铁路现实主义悬疑小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 岑清白; role: 主角; desire: 查清记录异常; fear: 错失真相; bottom_line: 不隐瞒现场数据; arc_start: 依赖规章; arc_end: 独立判断".to_string(),
            "name: 程清宁; role: 导师; desire: 平稳完成改造; fear: 旧事故重审; bottom_line: 不伪造巡道簿; arc_start: 固守经验; arc_end: 接受质疑".to_string(),
            "name: 祝怀禾; role: 对手; desire: 维持调度权威; fear: 失去控制; bottom_line: 不公开错误指令; arc_start: 推卸责任; arc_end: 被证据迫使让步".to_string(),
        ];
        let issues = vec!["ContractBlocker: 关系线角色 `工友群体` 不在角色权威表中".to_string()];
        let issues = typed_issues(ContractIssueKind::Governance, issues);

        let prompt = governance_patch_prompt(
            &draft,
            "其他内容你来决定",
            &issues,
            "稳定锚点",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(prompt.contains("relationship_ledger focused"), "{prompt}");
        assert!(
            prompt.contains("\"characters\":[\"岑清白\",\"程清宁\"]"),
            "{prompt}"
        );
        assert!(
            prompt.contains("\"characters\":[\"岑清白\",\"祝怀禾\"]"),
            "{prompt}"
        );
        assert!(prompt.contains("不得使用群体、组织"), "{prompt}");
    }

    #[test]
    fn staged_issue_focus_keeps_mixed_blockers_in_their_own_stage() {
        let mut issues = ContractIssueList::single(
            "contract.world_rules",
            ContractIssueKind::Governance,
            "world_rules",
            "ContractBlocker: 小说合同缺少世界规则",
        );
        issues.push_issue(ContractIssue::new(
            "contract.title",
            ContractIssueKind::Skeleton,
            ContractIssueEvidence::new("title", "missing"),
            "ContractBlocker: 小说合同缺少可锁定书名",
        ));
        issues.push_issue(ContractIssue::new(
            "contract.character_authority",
            ContractIssueKind::Characters,
            ContractIssueEvidence::new("characters", "bottom_line missing"),
            "ContractBlocker: 角色 `阮泊白`（主角）的底线锚点缺少明确边界、禁令或必须守住的行动",
        ));

        let skeleton = stage_relevant_contract_issues(ContractCompletionStage::Skeleton, &issues);
        assert!(skeleton.iter().any(|issue| issue.contains("书名")));
        assert!(!skeleton.iter().any(|issue| issue.contains("世界规则")));
        assert!(!skeleton.iter().any(|issue| issue.contains("底线锚点")));

        let characters =
            stage_relevant_contract_issues(ContractCompletionStage::Characters, &issues);
        assert!(characters.iter().any(|issue| issue.contains("底线锚点")));
        assert!(!characters.iter().any(|issue| issue.contains("世界规则")));
        assert!(!characters.iter().any(|issue| issue.contains("书名")));

        let governance =
            stage_relevant_contract_issues(ContractCompletionStage::Governance, &issues);
        assert!(governance.iter().any(|issue| issue.contains("世界规则")));
        assert!(!governance.iter().any(|issue| issue.contains("书名")));
        assert!(!governance.iter().any(|issue| issue.contains("底线锚点")));
    }

    #[test]
    fn plot_repair_prompt_requires_raw_outline_for_outline_pollution() {
        let draft = super::build_initial_creation_draft(
            "plot-outline-repair",
            "fiction",
            "写民国商战小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        let issues = typed_issues(
            ContractIssueKind::Plot,
            vec!["ContractBlocker: 小说合同大纲形成角色 `岑砚序` 与自身的关系变化".to_string()],
        );
        let prompt = plot_patch_prompt(
            &draft,
            "其他内容你来决定",
            &issues,
            "稳定锚点",
            "50000",
            "2500",
            20,
            "使用中文",
        );

        assert!(prompt.contains("必须提供完整修正后的 `raw_outline`"));
        assert!(prompt.contains("不能只返回 volumes"));
    }

    #[test]
    fn plot_repair_prompt_explains_how_to_separate_character_and_entity_names() {
        let draft = super::build_initial_creation_draft(
            "plot-character-entity-collision",
            "fiction",
            "写都市创业小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        let issues = typed_issues(
            ContractIssueKind::Plot,
            vec![
                "ContractBlocker: 小说合同大纲把角色权威名 `姜怀言` 用作组织、地点或机构名"
                    .to_string(),
            ],
        );
        let prompt = plot_patch_prompt(
            &draft,
            "继续修复当前合同",
            &issues,
            "稳定锚点",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(
            prompt.contains("保留该 canonical_name 作为人物姓名"),
            "{prompt}"
        );
        assert!(prompt.contains("为冲突的非人物实体改用一个"), "{prompt}");
        assert!(
            prompt.contains("raw_outline、volumes 和 near_chapters 中同步替换"),
            "{prompt}"
        );
    }

    #[test]
    fn plot_repair_prompt_explains_how_to_move_terminal_events_to_the_last_volume() {
        let draft = super::build_initial_creation_draft(
            "plot-longform-position",
            "fiction",
            "写校园青春小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        let issues = typed_issues(
            ContractIssueKind::Plot,
            vec![
                "ContractBlocker[outline.longform_position]: 小说合同非末卷提前完成权威终局"
                    .to_string(),
            ],
        );
        let prompt = plot_patch_prompt(
            &draft,
            "其他内容你来决定",
            &issues,
            "稳定锚点",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(prompt.contains("合并/删除单独尾声卷"), "{prompt}");
        assert!(
            prompt.contains("只有 volumes 数组的最后一个元素可以出现权威终局"),
            "{prompt}"
        );
    }

    #[test]
    fn plot_repair_prompt_moves_early_terminal_debt_while_restoring_terminal_coverage() {
        let draft = super::build_initial_creation_draft(
            "plot-terminal-coverage",
            "fiction",
            "写末日废土小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        let issues = typed_issues(
            ContractIssueKind::Plot,
            vec![
                "ContractBlocker[outline.terminal_coverage]: 小说合同末卷没有执行权威终局的核心解决事件"
                    .to_string(),
            ],
        );
        let prompt = plot_patch_prompt(
            &draft,
            "其他内容你来决定",
            &issues,
            "终局方向：主角关闭毒化阀门并公开账本",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(prompt.contains("终局方向”的全部核心行动"), "{prompt}");
        assert!(
            prompt.contains("不要为了字面相同而把完整终局复制"),
            "{prompt}"
        );
        assert!(
            prompt.contains("同义表达完成了终局方向中的选择"),
            "{prompt}"
        );
        assert!(
            prompt.contains("把该非末卷改成尚未完成终局的准备、代价或未解决债务"),
            "{prompt}"
        );
        assert!(
            prompt.contains("不能把其中的关键人物、物件、机制、行动或结果改成较弱"),
            "{prompt}"
        );
    }

    #[test]
    fn semantic_outline_authority_prompt_repairs_volume_and_payoff_sequence_together() {
        let draft = super::build_initial_creation_draft(
            "plot-semantic-sequence-conflict",
            "fiction",
            "写修仙小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        let issues = typed_issues(
            ContractIssueKind::Plot,
            vec![
                "ContractBlocker[semantic.outline_character_authority]: 第2卷卷尾变化中陶泊衡燃尽修为，与第2卷目标中对抗闻云澜夺回灵脉的逻辑顺序冲突；卷尾变化直接跳到终局状态动作。权威证据 终局方向=`陶泊衡燃尽修为点燃青灯`；候选证据 第2卷卷尾变化=`陶泊衡燃尽修为，世界灵气重新流动`"
                    .to_string(),
            ],
        );
        let prompt = plot_patch_prompt(
            &draft,
            "其他内容你来决定",
            &issues,
            "终局方向：陶泊衡燃尽所有修为，以身为引点燃青灯，将枯竭的灵气重新注入大地",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(prompt.contains("同一个 Plot 因果链问题"), "{prompt}");
        assert!(
            prompt.contains("同步重写被点名 volume 的 objective 与 ending_change"),
            "{prompt}"
        );
        assert!(
            prompt.contains("payoff_matrix 中镜像同一错误终局跳跃的 payoff_target 也必须一并改写"),
            "{prompt}"
        );
        assert!(prompt.contains("\"payoff_matrix\""), "{prompt}");
        assert!(prompt.contains("\"volumes\""), "{prompt}");
    }

    #[test]
    fn plot_repair_focuses_only_near_chapters_when_other_plot_fields_are_stable() {
        let draft = super::build_initial_creation_draft(
            "plot-near-chapters-focused",
            "fiction",
            "写历史架空小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        let issues = vec![
            "ContractBlocker: 小说合同近期章节包缺少第1章目标，不能进入写作确认".to_string(),
            "ContractBlocker: 小说合同近期章节编号必须从第1章开始连续递增，不能跳号、重号或乱序"
                .to_string(),
        ];
        let issues = typed_issues(ContractIssueKind::Plot, issues);

        let prompt = plot_patch_prompt(
            &draft,
            "其他内容你来决定",
            &issues,
            "稳定锚点",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(prompt.contains("near_chapters focused"), "{prompt}");
        assert!(
            prompt.contains("只完整替换 `outline.near_chapters`"),
            "{prompt}"
        );
        assert!(prompt.contains("\"number\":1"), "{prompt}");
        assert!(prompt.contains("\"number\":3"), "{prompt}");
        assert!(!prompt.contains("\"volumes\""), "{prompt}");
        assert!(!prompt.contains("\"payoff_matrix\""), "{prompt}");
        assert!(!prompt.contains("\"raw_outline\""), "{prompt}");
    }

    #[test]
    fn plot_near_chapter_focus_does_not_hide_other_plot_repairs() {
        let issues = vec![
            "ContractBlocker: 小说合同近期章节编号必须从第1章开始连续递增".to_string(),
            "ContractBlocker: 小说合同分卷规划含有结构污染或无效卷名".to_string(),
        ];
        assert!(!plot_issue_focus_is_only_near_chapters(&issues));
    }

    #[test]
    fn explicit_numbered_chapter_revision_uses_full_plot_repair() {
        let draft = super::build_initial_creation_draft(
            "plot-explicit-numbered-chapter-revision",
            "fiction",
            "写历史架空小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        let issues = vec![
            "ContractBlocker[contract.explicit_revision]: 用户明确合同修订尚未经过对应 typed patch 实际写入：第1章保持主角是尚未继位的皇子；第5章写明其父皇驾崩后由该皇子继位"
                .to_string(),
        ];
        let issues = typed_issues(ContractIssueKind::Plot, issues);

        let prompt = plot_patch_prompt(
            &draft,
            "继续修改合同，不写正文",
            &issues,
            "稳定锚点",
            "100000",
            "2500",
            40,
            "使用中文",
        );

        assert!(!prompt.contains("near_chapters focused"), "{prompt}");
        assert!(prompt.contains("\"volumes\""), "{prompt}");
        assert!(prompt.contains("\"raw_outline\""), "{prompt}");
    }

    #[test]
    fn explicit_numbered_chapter_revision_keeps_full_plot_repair_when_other_scope_is_named() {
        let issues = vec![
            "ContractBlocker[contract.explicit_revision]: 修改第5章，并同步重写全书大纲与分卷规划"
                .to_string(),
        ];

        assert!(!plot_issue_focus_is_only_near_chapters(&issues));
    }

    #[test]
    fn governance_schema_uses_locked_character_names_instead_of_role_placeholders() {
        let mut draft = super::build_initial_creation_draft(
            "governance-schema-authority",
            "fiction",
            "写都市职场小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "name: 秦知安; role: 主角; desire: 守住职业尊严; fear: 被制度吞没; bottom_line: 不伪造证据; arc_start: 习惯退让; arc_end: 主动公开真相".to_string(),
            "name: 岑予晚; role: 盟友; desire: 查清合同黑箱; fear: 再次失去信任; bottom_line: 不利用同伴; arc_start: 保持距离; arc_end: 共同承担".to_string(),
            "name: 辛砚序; role: 关键对手; desire: 维持资源垄断; fear: 账本公开; bottom_line: 不交出控制权; arc_start: 操控局面; arc_end: 被证据逼到台前".to_string(),
        ];

        let issues = typed_issues(
            ContractIssueKind::Governance,
            vec![
                "ContractBlocker: 小说合同缺少世界规则".to_string(),
                "ContractBlocker: 小说合同缺少关系账本".to_string(),
            ],
        );
        let prompt = governance_patch_prompt(
            &draft,
            "其他内容你来决定",
            &issues,
            "稳定锚点",
            "50000",
            "2500",
            20,
            "使用中文",
        );

        assert!(
            prompt.contains("\"characters\":[\"秦知安\",\"岑予晚\"]"),
            "{prompt}"
        );
        assert!(prompt.contains("\"name\":\"辛砚序\""), "{prompt}");
        assert!(!prompt.contains("角色A"));
        assert!(!prompt.contains("角色B"));
    }
}
