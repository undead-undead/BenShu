use super::{
    model::{is_chinese_language, required_memo_sections, ChapterMemo},
    protocol::{CharacterAuthority, RevisionMode},
};
use crate::tool::writing::novel_governance as governance;

pub(crate) fn chapter_execution_prompt(
    language: &str,
    title: &str,
    chapter_number: usize,
    context_json: &str,
    previous_error: Option<&str>,
) -> String {
    let sections = required_memo_sections(language).join("\n- ");
    let rolling_lookahead = governance::ROLLING_OUTLINE_LOOKAHEAD_CHAPTERS;
    let retry = previous_error
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            format!("\n\n上一轮问题 / Previous issue:\n{value}\n请修复后重新输出完整 JSON。")
        })
        .unwrap_or_default();
    if is_chinese_language(language) {
        format!(
            "为《{title}》生成第 {chapter_number} 章的章节执行包。\n\n上下文包：\n{context_json}\n\n只输出一个有效、紧凑的 JSON 对象，不要 Markdown 代码块；不得为了缩短输出而省略字段。字段必须齐全：memo_markdown, architecture, scene_goal, conflict, choice, cost, reveal, emotional_beat, chapter_function, irreversible_event, new_state_after_chapter, world_change, character_change, relationship_change, power_delta, resource_delta, hook_opened, hook_paid_off, title_basis, future_chapters, new_character_requests。无变化的字符串填空字符串，数组填空数组。\n\nmemo_markdown 必须是对象：{{\"goal\":\"一句话目标\",\"sections\":{{...}}}}；sections 必须逐项包含以下键，每项只写一句：\n- {sections}\narchitecture 必须是恰好 5 个短场景节点的数组；每项写清场景功能、人物行动、继承事实和状态变化。使用对象/数组而不是带换行的长 JSON 字符串，避免转义损坏。\n\n强类型变化只写本章正文可证明的事实。scene_goal/conflict/choice/cost/reveal/emotional_beat 必须具体；world_change/power_delta/resource_delta、hook_opened/hook_paid_off 没有真实变化就留空；new_state_after_chapter 必须写清本章权威目标完成后的可验证章末状态并点名受影响的既有权威实体；chapter_function、irreversible_event、new_state_after_chapter、world_change、character_change、relationship_change 和 title_basis 必须相互一致并服从 narrative_progress、下一章边界和结局方向。\n\nfuture_chapters 用于补充当前章之后最多 {rolling_lookahead} 个尚未锁定的滚动大纲节点；每项严格使用 {{\"number\":整数,\"goal\":\"具体事件目标\",\"expected_turn\":\"本章末不可逆变化\"}}。每个节点必须从前一节点的新状态继续，事件彼此不同，不得重放 recent_approved_chapters 已完成的发现、抵达、取得、会面、决定或状态变化；上下文已有的章节目标属于只读权威，不得改写。每个未来节点必须位于 project.volumes 为该章编号划定的分卷内，只写可在单章完成的局部步骤；不得把整卷 objective/ending_change 复制成单章，不得在后续分卷的 start_chapter 之前执行其目标或卷尾变化。若已到全书结尾或未来节点均已存在，输出空数组。\n\nnew_character_requests 只声明确有用途的新人物，不命名，使用稳定 ASCII request_id；没有新人物时为空数组。继承既有角色、世界规则、伏笔、节奏与连续性，不重命名，不写正文，不输出流程说明。目标语言：{language}。{retry}"
        )
    } else {
        format!(
            "Create the chapter {chapter_number} execution package for \"{title}\". Return only one valid compact JSON object with every field below; never omit a field to shorten the response: memo_markdown, architecture, scene_goal, conflict, choice, cost, reveal, emotional_beat, chapter_function, irreversible_event, new_state_after_chapter, world_change, character_change, relationship_change, power_delta, resource_delta, hook_opened, hook_paid_off, title_basis, future_chapters, new_character_requests. Use empty strings or arrays when no change exists.\n\nContext package:\n{context_json}\n\nmemo_markdown must be an object shaped as {{\"goal\":\"one sentence\",\"sections\":{{...}}}}. Its sections object must contain each key below with one sentence per value:\n- {sections}\narchitecture must be an array of exactly 5 short scene beats, each stating function, action, inherited fact, and resulting state. Prefer objects and arrays over newline-heavy JSON strings.\n\nOnly declare changes provable in this chapter. Leave world_change, power_delta, resource_delta, hook_opened, and hook_paid_off empty when no such durable change occurs. new_state_after_chapter must name the affected existing authority entity and the verifiable end state after the current authoritative chapter goal is completed. Keep every typed field mutually consistent with narrative_progress, the next-chapter boundary, and the ending. future_chapters may add at most {rolling_lookahead} still-unlocked outline nodes after this chapter, each shaped as {{\"number\":integer,\"goal\":\"concrete event goal\",\"expected_turn\":\"irreversible end change\"}}. Continue each node from the prior new state, keep all events distinct, never replay a discovery, arrival, acquisition, meeting, decision, or state change already completed in recent_approved_chapters, and never replace an existing chapter goal. Every future node must stay within the project.volumes range assigned to its chapter number and describe a local step that one chapter can complete; never copy an entire volume objective or ending_change into one chapter, and never execute a later volume before its start_chapter. Return an empty array at the ending or when all future nodes already exist. new_character_requests contains only genuinely needed unnamed characters with stable ASCII request IDs; otherwise use an empty array. Preserve names, rules, hooks, pacing, and continuity. Do not write prose or workflow notes. Target language: {language}.{retry}"
        )
    }
}

pub(crate) fn writer_prompt(
    language: &str,
    title: &str,
    chapter_number: usize,
    chapter_target: Option<usize>,
    memo: &ChapterMemo,
    architecture: &str,
    context_json: &str,
    authority: &CharacterAuthority,
) -> String {
    let output_contract = writer_output_contract_instruction(language, chapter_target, true);
    let stream_protocol = draft_stream_protocol_instruction(language);
    let anchors = contract_anchor_instruction(language, authority);
    if is_chinese_language(language) {
        format!(
            "为《{title}》写第 {chapter_number} 章正文。\n\n{output_contract}\n\n{stream_protocol}\n\n{anchors}\n\n章节 memo：\n{}\n\n章节架构与执行合同：\n{architecture}\n\n上下文包：\n{context_json}\n\n标题必须根据本章执行合同里的 title_basis / irreversible_event / 大纲节点来取，不能复用近章标题，也不能堆叠与本章证据无关的抽象气氛词。标题和正文都必须使用中文；必须原样使用合同中的角色名和专有名，不要音译、翻译、改名或临时创造替代主角。同一关键物件的来源、持有者、位置、状态和首次获得事件必须前后一致；若正文存在两个相似物件，必须明确区分，不能把已经持有的物件再次写成首次获得。正文必须完整，不要省略、不要占位、不要写内部说明。摘要、关键事实和连续性元数据由系统在最终正文之后结算，不要混入正文输出。",
            memo.body
        )
    } else {
        format!(
            "Write chapter {chapter_number} for \"{title}\".\n\n{output_contract}\n\n{stream_protocol}\n\n{anchors}\n\nChapter memo:\n{}\n\nArchitecture and execution contract:\n{architecture}\n\nContext package:\n{context_json}\n\nThe title must come from the execution contract's title_basis / irreversible_event / outline node; do not reuse recent chapter titles or rely on abstract mood-word stacking. Preserve contract names and proper nouns exactly; do not translate, transliterate, rename, or invent a substitute protagonist. Keep every key object's origin, holder, location, state, and first-acquisition event consistent; if two similar objects exist, distinguish them explicitly instead of reacquiring an already-held object. The body must be complete prose with no omissions, placeholders, or workflow notes. Summary, key facts, and continuity metadata are settled by the system from the final body; do not mix them into the prose output.",
            memo.body
        )
    }
}

fn draft_stream_protocol_instruction(language: &str) -> &'static str {
    if is_chinese_language(language) {
        "使用流式安全的纯文本协议输出，正文不得嵌入 JSON 字符串，也不要使用 Markdown 代码块：\nTITLE: <章节标题>\n---BODY---\n<完整章节正文>\n---END BODY---\n最后一行终止标记必须在正文完整结束后输出。"
    } else {
        "Use this stream-safe plain-text protocol. Do not embed the body in a JSON string and do not use a Markdown code fence:\nTITLE: <chapter title>\n---BODY---\n<complete chapter prose>\n---END BODY---\nEmit the final marker only after the body is complete."
    }
}

fn writer_output_contract_instruction(
    language: &str,
    chapter_target: Option<usize>,
    first_draft: bool,
) -> String {
    match (is_chinese_language(language), chapter_target) {
        (true, Some(target)) if target > 0 => {
            if first_draft {
                let first_draft_target =
                    target.saturating_mul(110).div_ceil(100).max(target);
                format!(
                    "输出合同：content 正文不得少于 {target} 个中文非空白字符。这个数值来自面板/worker 的章节最低目标；低于该值视为本章未完成。首次草稿请直接写到约 {first_draft_target} 字，为流式截断和清理预留余量，不要依赖后续补写。"
                )
            } else {
                format!(
                    "输出合同：修订后的 content 正文仍不得少于 {target} 个中文非空白字符，不得通过删减正文规避问题。"
                )
            }
        }
        (false, Some(target)) if target > 0 => {
            if first_draft {
                let first_draft_target =
                    target.saturating_mul(110).div_ceil(100).max(target);
                format!(
                    "Output contract: content must contain at least {target} substantive non-whitespace units. This is the panel/worker minimum. Aim for about {first_draft_target} units in the first draft to leave room for streaming truncation and cleanup instead of relying on a later expansion pass."
                )
            } else {
                format!(
                    "Output contract: revised content must still contain at least {target} substantive non-whitespace units; do not evade an issue by shortening the body."
                )
            }
        }
        (true, _) => "输出合同：content 必须是完整章节正文，长度由当前任务和章节架构自然决定，不要省略、占位或写流程说明。".to_string(),
        (false, _) => "Output contract: content must be a complete chapter draft sized naturally for the task and architecture, with no omissions, placeholders, or workflow notes.".to_string(),
    }
}

pub(crate) fn reviser_prompt(
    language: &str,
    title: &str,
    chapter_number: usize,
    chapter_target: Option<usize>,
    memo: &ChapterMemo,
    architecture: &str,
    context_json: &str,
    content: &str,
    issues: &[String],
    mode: RevisionMode,
    authority: &CharacterAuthority,
) -> String {
    let issues = render_list(issues);
    let output_contract = writer_output_contract_instruction(language, chapter_target, false);
    let stream_protocol = draft_stream_protocol_instruction(language);
    let anchors = contract_anchor_instruction(language, authority);
    let same_chapter_rewrite = mode == RevisionMode::FullRewrite;
    if is_chinese_language(language) {
        if same_chapter_rewrite {
            return format!(
                "重写《{title}》第 {chapter_number} 章正文。上一版正文存在结构性退化，请按同一章合同从头生成一版完整正文；这不是另开新章，必须保持同一章目标、角色、设定、阶段转折和结局方向。\n\n{output_contract}\n\n{stream_protocol}\n\n{anchors}\n\n章节 memo：\n{}\n\n章节架构：\n{architecture}\n\n只读章节权威包：\n{context_json}\n\n必须修复的问题：\n{issues}\n\n旧稿处理原则：不要复用上一版正文的段落顺序、重复句、坏尾句或无行动推进段落；不要把旧稿当作续写开头，只按合同重新成章。\n\n标题和正文必须使用中文；必须保留合同中的角色名和专有名，不要音译、翻译、改名或替换主角。同一关键物件的来源、持有者、位置、状态和首次获得事件必须前后一致，两个相似物件必须明确区分。正文必须完整，必须写出具体行动、代价、关系变化和章尾新状态；不要写“修订后的内容如下”这类说明。",
                memo.body
            );
        }
        format!(
            "修订《{title}》第 {chapter_number} 章。只修复列出问题，不要重写成另一章。\n\n{output_contract}\n\n{stream_protocol}\n\n{anchors}\n\n章节 memo：\n{}\n\n章节架构：\n{architecture}\n\n只读章节权威包：\n{context_json}\n\n问题：\n{issues}\n\n原正文：\n{content}\n\n标题和正文必须使用中文；必须保留合同中的角色名和专有名，不要音译、翻译、改名或替换主角。同一关键物件的来源、持有者、位置、状态和首次获得事件必须前后一致，两个相似物件必须明确区分。正文是修订后的完整正文，不要写“修订后的内容如下”这类说明。",
            memo.body
        )
    } else {
        if same_chapter_rewrite {
            return format!(
                "Rewrite chapter {chapter_number} of \"{title}\". The previous body has structural degradation, so generate a fresh complete body for the same chapter contract. This is not a new chapter: preserve the same chapter goal, characters, rules, phase turn, and ending direction.\n\n{output_contract}\n\n{stream_protocol}\n\n{anchors}\n\nChapter memo:\n{}\n\nArchitecture:\n{architecture}\n\nRead-only chapter authority:\n{context_json}\n\nIssues to fix:\n{issues}\n\nOld-draft rule: do not reuse the previous prose's paragraph order, repeated sentences, bad ending, or actionless passages; do not treat the old draft as a continuation seed. Rebuild the same chapter from the contract.\n\nPreserve contract names and proper nouns exactly; do not translate, transliterate, rename, or replace the protagonist. Keep every key object's origin, holder, location, state, and first-acquisition event consistent, and explicitly distinguish similar objects. The body must be complete prose with concrete action, cost, relationship change, and a new end-state; no revision-note preface.",
                memo.body
            );
        }
        format!(
            "Revise chapter {chapter_number} of \"{title}\". Fix the listed issues without turning it into a different chapter.\n\n{output_contract}\n\n{stream_protocol}\n\n{anchors}\n\nChapter memo:\n{}\n\nArchitecture:\n{architecture}\n\nRead-only chapter authority:\n{context_json}\n\nIssues:\n{issues}\n\nOriginal prose:\n{content}\n\nPreserve contract names and proper nouns exactly; do not translate, transliterate, rename, or replace the protagonist. Keep every key object's origin, holder, location, state, and first-acquisition event consistent, and explicitly distinguish similar objects. The body is the complete revised prose, with no revision-note preface.",
            memo.body
        )
    }
}

fn contract_anchor_instruction(language: &str, authority: &CharacterAuthority) -> String {
    let names = &authority.canonical_names;
    if names.is_empty() {
        if is_chinese_language(language) {
            return "角色合同：使用上下文包中的原始角色名，禁止音译、翻译、改名。".to_string();
        }
        return "Character contract: use the exact original names from the context package; do not translate, transliterate, or rename them.".to_string();
    }
    if is_chinese_language(language) {
        format!(
            "角色合同：本章当前允许出场的稳定角色名是：{}。权威主角：{}。整章必须至少出现一个稳定角色名；禁止音译、翻译、改名或临时创造替代主角。除上述稳定角色和章节架构中由系统明确分配的角色名外，不得自行给新人物起名；临时功能人物只能使用身份称谓。其他长期合同角色若没有在本章 memo 或架构中明确要求出场，只属于未来，不得让其登场、说话、行动或提前揭示其信息。",
            names.join("、"),
            authority.protagonist.as_deref().unwrap_or("未单独指定")
        )
    } else {
        format!(
            "Character contract: the stable characters currently allowed to appear in this chapter are: {}. Authoritative protagonist: {}. The chapter must mention at least one stable character name. Do not translate, transliterate, rename, or invent a substitute protagonist. Do not name any new character unless the system-assigned name appears explicitly in the chapter architecture; use role descriptions for incidental functional figures. Other long-term contract characters belong to the future unless this chapter memo or architecture explicitly requires them; do not make them appear, speak, act, or reveal information early.",
            names.join(", "),
            authority.protagonist.as_deref().unwrap_or("not separately specified")
        )
    }
}

fn render_list(items: &[String]) -> String {
    if items.is_empty() {
        return "- none".to_string();
    }
    items
        .iter()
        .filter(|item| !item.trim().is_empty())
        .map(|item| format!("- {}", item.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn final_chapter_observer_prompt(
    language: &str,
    chapter_number: usize,
    authority_context: &str,
    content: &str,
    previous_error: Option<&str>,
) -> String {
    let retry = previous_error
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            format!(
                "\n\n上一轮状态结算未通过 / Previous settlement errors:\n{value}\n只修正观察结果的证据、ID、路径和字段，不得虚构正文事件。"
            )
        })
        .unwrap_or_default();
    if is_chinese_language(language) {
        format!(
            "你是小说状态观察器。只根据已经定稿的第 {chapter_number} 章正文结算章末状态；写作阶段的 summary、key_facts、continuity_updates 或执行包声明都不是事实来源。\n\n\
             项目权威上下文仅用于识别既有实体、合同变化和伏笔，不得用它补写正文里没有发生的事件：\n{authority_context}\n\n\
             最终正文：\n{content}\n\n\
             只输出一个紧凑 JSON 对象，字段固定为 current_state, pending_hooks, chapter_summary, continuity_updates, resolved_hooks, state_changes。current_state 和 chapter_summary 各用一句话；current_state 必须写清章末地点、当前冲突或异常是持续还是解除、主角代价或身体/能力状态，以及正文涉及的关键物件或关系状态，不适用的维度省略。continuity_updates 最多 4 项，全部只写正文可见事实。resolved_hooks 只列正文明确兑现的既有伏笔；pending_hooks 只写章末仍开放的明确线索，没有则为空字符串。\
             state_changes 是唯一可持久化的 typed delta。每项只需 entity_id, event_type, value, evidence, authority_path, authority_excerpt；evidence 只输出 {{\"excerpt\":\"正文中唯一出现、包含对应人物或权威实体表面名的连续短片段\"}}。片段优先使用一个完整句子；如果权威变化由相邻句子共同完成，可以连续选取最多 3 个相邻句子，但不得跨段且不得超过 320 个字符。字符偏移和 change_id 由本地验证器绑定，不要计算。event_type 只能是 character、relationship、world、power、resource、hook_seed、hook_advance、hook_pay_off、hook_defer、incidental。value 必须与 evidence.excerpt 完全相同，不得概括或改写。entity_id 使用稳定 ID；没有稳定 ID 的物件、地点或其他权威实体使用合同中的精确表面名。\
             chapter_contract.new_state_after_chapter 是本章必须由最终正文实现的权威章末变化：必须从正文选取一段唯一、点名对应权威实体的连续原文证据，按实际变化类型输出一项 authority_path=chapter_contract.new_state_after_chapter 的 typed delta；一条句子不能完整证明时，使用上述最多 3 个相邻句子的短片段。如果正文没有实现它，不得伪造，保持缺失以让本地验证器阻断。其余非空的 character_change、relationship_delta、world_change、power_delta、resource_delta、hook_opened/N、hook_paid_off/N 是允许上限，不是强制全部发生的清单。只为最终正文明确实现且有唯一连续原文证据的项目输出变化：authority_path 使用该精确路径，authority_excerpt 逐字复制该合同字段。如果某项变化与 new_state_after_chapter 指向同一实体的同一变化类型，只保留 new_state_after_chapter 这一项，不得再输出会覆盖它的平行 delta。正文未实现或证据不够明确时必须省略，不得为了填满合同字段而伪造证据。若 new_state_after_chapter 或 payoff_target 对既有伏笔形成唯一指向且正文明确推进它，额外输出 hook_advance；多义时不得猜测。延后伏笔只能使用精确指向既有伏笔的 chapter_contract.payoff_target。incidental 使用 bounded_incidental；只有某个风险布尔值确为 true 时才输出该布尔字段，否则全部省略。hook_defer 必须给出晚于当前章的 defer_until_chapter。\
             使用正文原始姓名和专名；不要复制超过证据上限的长段正文，不要推测幕后真相，不要 Markdown，不要说明。严格形状示例：{{\"current_state\":\"一句话\",\"pending_hooks\":\"一句话或空字符串\",\"chapter_summary\":\"一句话\",\"continuity_updates\":[],\"resolved_hooks\":[],\"state_changes\":[{{\"entity_id\":\"character-0001\",\"event_type\":\"character\",\"value\":\"正文中唯一连续原文片段\",\"evidence\":{{\"excerpt\":\"正文中唯一连续原文片段\"}},\"authority_path\":\"chapter_contract.new_state_after_chapter\",\"authority_excerpt\":\"合同章末变化原文\"}}]}}。没有对应项时数组为空。{retry}"
        )
    } else {
        format!(
            "You are a fiction state observer. Settle the end state of final chapter {chapter_number} from final prose only. Writer-stage summaries and execution-package claims are not evidence.\n\n\
             Project authority is only for identifying established entities, contracted changes, and hooks; never invent an event absent from the prose:\n{authority_context}\n\n\
             Final prose:\n{content}\n\n\
             Return one compact JSON object with exactly current_state, pending_hooks, chapter_summary, continuity_updates, resolved_hooks, and state_changes. Keep current_state and chapter_summary to one sentence each. current_state must state the end location, whether the active conflict or anomaly remains active or is resolved, the protagonist's cost or physical/ability condition, and any key object or relationship state present in the prose; omit dimensions that do not apply. Keep continuity_updates to at most four visibly supported facts. resolved_hooks contains only established hooks explicitly paid off in the body; pending_hooks contains only explicit open clues, or an empty string.\
             state_changes is the only durable typed delta. Each item needs only entity_id, event_type, value, evidence, authority_path, and authority_excerpt. evidence must be {{\"excerpt\":\"a short uniquely occurring contiguous verbatim body excerpt containing the corresponding public authority-entity surface\"}}. Prefer one complete sentence. When adjacent sentences jointly realize the authority change, select at most three adjacent sentences from the same paragraph and no more than 320 characters. The local validator binds character offsets and change_id, so do not calculate them. value must equal evidence.excerpt exactly; never summarize or paraphrase it. Use stable entity IDs. When an authoritative object, place, or other entity has no stable ID, use its exact contract surface.\
             chapter_contract.new_state_after_chapter is the authoritative end-state change that final prose must realize. Select one unique contiguous verbatim body excerpt naming the corresponding authority entity and emit one typed delta with authority_path=chapter_contract.new_state_after_chapter and the actual event type. If one sentence cannot prove the complete change, use the bounded adjacent-sentence excerpt described above. If the body did not realize it, do not invent evidence; leave it missing so local validation blocks approval. If another contract field describes the same event type for the same entity, emit only the new_state_after_chapter delta instead of a parallel delta that would overwrite it. When new_state_after_chapter or payoff_target uniquely identifies an existing hook and the final body visibly advances it, emit an additional hook_advance delta; never guess when multiple hooks could match.\
             Non-empty chapter_contract character_change, relationship_delta, world_change, power_delta, resource_delta, hook_opened/N, and hook_paid_off/N fields are the maximum allowed changes, not a checklist that must all occur. Emit a matching typed delta only when the final body explicitly realizes it with one unique contiguous verbatim evidence excerpt; use the exact authority_path and copy the contract field verbatim into authority_excerpt. Omit unrealized or weakly evidenced changes instead of inventing evidence. Use chapter_contract.payoff_target for advancing or deferring an existing hook only when it names that exact hook. incidental uses bounded_incidental; omit all risk booleans unless one is actually true. hook_defer also requires a later defer_until_chapter. Preserve exact names. Do not copy evidence longer than the stated bound, infer hidden truths, use Markdown, or add commentary. Exact shape example: {{\"current_state\":\"one sentence\",\"pending_hooks\":\"one sentence or empty\",\"chapter_summary\":\"one sentence\",\"continuity_updates\":[],\"resolved_hooks\":[],\"state_changes\":[{{\"entity_id\":\"character-0001\",\"event_type\":\"character\",\"value\":\"the exact same unique contiguous verbatim body excerpt\",\"evidence\":{{\"excerpt\":\"the exact same unique contiguous verbatim body excerpt\"}},\"authority_path\":\"chapter_contract.new_state_after_chapter\",\"authority_excerpt\":\"verbatim contract end-state field\"}}]}}. Use empty arrays when no item exists.{retry}"
        )
    }
}
