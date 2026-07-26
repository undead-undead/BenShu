# BenShu 小说权威与有限修订重构实施核对记录

> 对照文档：`BENSHU_NOVEL_AUTHORITY_AND_REVISION_REFACTOR_PLAN_ZH.md`
>
> 范围：实施前置 A、Phase 1～5。Phase 6 明确排除，未实施独立审稿模型。

## 1. 实施边界与基线

- 基线提交：`5a3ce83 refactor(writing): unify novel authority and revision flow`
- 运行数据库 `data/cron.redb` 不属于本重构，未纳入提交。
- 写作领域 owner 保持在 `crates/builtin-tools/src/tool/writing`。
- 网关只负责通用 durable task 的 provider 恢复调度；不持有小说合同、命名、审稿或状态语义。
- 决策顺序：复用现有 owner，原位修正缺陷，接通后删除旧生产路径；仅在没有现有能力时新增持久化记录。

## 2. 实施前置 A：合同有限收敛

| 字段 | 核对结果 |
| --- | --- |
| 目标不变量 | 本地确定性修复每类最多一次；LLM semantic patch 默认一次、绝对最多两次；`Uncertain` 仅 advisory；确认合同原子落盘。 |
| 当前 owner | `creation_contract` 的 typed issue、candidate ranking、repair progress、staged repair 与 pending candidate。 |
| 复用决定 | 复用 `ContractReadinessScope`、`PatchFieldStrength`、typed patch、`ContractRepairProgress`、本地命名治理。 |
| 修正/替换 | semantic patch 预算收口到两次；冲突必须有双侧精确证据；runtime/parse failure 不伪装成合同冲突；项目先在临时目录完成，再原子切换。 |
| 删除清单 | 30 轮总修复常量与循环、依靠 issue 自然语言选择 owner 的生产分流、失败后可见的半初始化项目。 |
| 兼容边界 | 历史 issue 文本可展示，但不再决定 hard/soft 或 patch owner。 |
| 验证证据 | 合同预算、候选净提升、`Uncertain`、用户权威保持、任意总字数与 2500/5000 档测试。 |

结论：已对齐。合同阶段不会进入 3～30 次语义自修，也不会因同模型主观不确定而阻止结构完整合同确认。

## 3. Phase 1：唯一 sealed chapter authority

| 字段 | 核对结果 |
| --- | --- |
| 目标不变量 | Writer、Auditor、Reviser、Observer 消费同一个 sealed root；每个 role 只有可追踪投影。 |
| 当前 owner | `novel_governance::SealedChapterAuthority`、`chapter_planning`、`context_packaging`。 |
| 复用决定 | 复用现有 context package、rule stack、trace、chapter contract 与 architecture record。 |
| 修正/替换 | schema v2；canonical contract 与 as-of truth 写入 root；role projection 记录 included/excluded/truncated paths、coverage 与独立 fingerprint；stage record 验证同 root。 |
| 冲突路径处理 | 合同或已批准 truth 变化使未批准后代统一 stale；恢复不得重建另一执行包；minimal/degraded context 不得进入正常写作或批准。 |
| 删除清单 | 各角色重新 compose 权威、恢复链重生成 execution package、缺权威继续写作的 fallback。 |
| 验证证据 | projection coverage、截断、stale authority、同 root、旧 schema 重建与越界路径测试。 |

结论：已对齐。受保护权威不因 prompt 预算被静默截断，压缩只作用于可压缩上下文。

## 4. Phase 2：证据化 typed hard gate

| 字段 | 核对结果 |
| --- | --- |
| 目标不变量 | 只有确定性 schema/合同/连续性/状态证据能够 hard block；score 与自由文本仅 advisory。 |
| 当前 owner | `chapter_quality` typed finding、`novel_workflow_driver::quality` 与统一 disposition。 |
| 复用决定 | 复用本地机械检查、authority fingerprint、body fingerprint 与现有 audit receipt。 |
| 修正/替换 | finding 必须携带 code/class/source/disposition/evidence grade 及依赖指纹；语义冲突要求 authority/body 双侧证据；伪造 path 不能升级 hard。 |
| 删除清单 | `audit_issue_is_actionable`、hard-marker 字符串分类、仅凭 free text/score 触发修订或阻断的路径。 |
| 验证证据 | 主观低分、warning-only、JSON parse failure、无双侧证据、伪造 authority 引用与同 finding 跨入口一致性测试。 |

结论：已对齐。hard blocker 不能因预算耗尽被放行，主观建议也不能启动正文重写。

## 5. Phase 3：单一有限修订与 best candidate

| 字段 | 核对结果 |
| --- | --- |
| 目标不变量 | 新写、恢复、旧草稿、外层 retry 与 regenerate 共用一个 bounded controller 和一个 best selector。 |
| 当前 owner | `novel_workflow_driver::chapter_loop`、`chapter_runtime`、`DraftCandidateRecord`。 |
| 复用决定 | 复用净提升比较、最佳版本回滚、local cleanup、length top-up 与 tail completion。 |
| 修正/替换 | 完整 `DraftOutput`、typed findings、provenance、quality vector、body/metadata/authority 指纹进入同一 candidate；semantic revision 绝对最多两次；accepted best 持久化到单一 canonical best 文件。 |
| 防删除取巧 | required outcome、保护事实、受保护锚点及大段 material deletion 进入质量向量，删剧情不能取得净提升。 |
| 正文协议 | 长正文以标题/正文流为主；metadata 可后置确定性修复，不要求 Writer 为缺 metadata 重写正文。 |
| 字数边界 | 2500 档 hard max 5000；5000 档 hard max 10000；总字数保持任意合同值。 |
| 删除清单 | 恢复专用语义循环、独立 stalled 计数、按最长正文猜 best、旧 candidate 恢复 selector。 |
| 验证证据 | 两次预算、无净提升停止、best 崩溃恢复、保护事实删除、free-text 不路由及两档 hard max 测试。 |

结论：已对齐。预算耗尽保留 best pending，而不是继续盲修或伪批准。

## 6. Phase 4：最终正文 typed settlement 与原子批准

| 字段 | 核对结果 |
| --- | --- |
| 目标不变量 | durable state 只从最终正文的 typed delta 结算；批准事务只能恢复成完整未提交或完整已提交。 |
| 当前 owner | `settlement`、`state_truth`、`novel_bible` typed reducer、`approval_transaction`、`snapshot`。 |
| 复用决定 | 复用现有 pending settlement、Story Bible、staging/backup snapshot 和 approval receipt。 |
| 修正/替换 | display summary/key facts/continuity 不再生成 durable truth；重大人物/世界/关系/伏笔变化必须有 final-body evidence 且获 chapter contract 许可；失败标记 state-degraded 并保留旧 truth。 |
| 伏笔生命周期 | seed/advance/payoff/defer/overdue 使用 typed hook ID、目标与证据；常见词和 bigram 不能授权状态变化。 |
| 批准事务 | prepared journal 前保存 before image；重启时重新验证 accepted best、settlement、body、authority、truth；未批准回滚，已批准完成 receipt；重放不重复应用 delta。 |
| 删除清单 | 从自然语言 metadata 推人物/世界状态的 reducer、批准前同步 metadata 到 truth、无依赖校验的轻量 snapshot 成功路径。 |
| 验证证据 | metadata 不污染状态、final-body evidence、超合同 delta、settlement degraded、receipt 指纹、snapshot 缺件与事务恢复测试。 |

结论：生产路径已对齐；旧测试夹具已改为先建立 sealed authority、accepted best 和 settlement，再验证批准顺序。

## 7. Phase 5：兼容、删除、rolling batch 与恢复

| 字段 | 核对结果 |
| --- | --- |
| 目标不变量 | 旧数据只读迁移后进入新 owner；进度以磁盘连续批准正文为准；durable goal 跨 batch/进程/provider 恢复。 |
| 当前 owner | `project_config` migration、`state_truth` repair、`snapshot`、现有项目 lease、`project_goal`、`ContinuousTaskExecutor` 与网关 durable rescheduler。 |
| 旧未批准章节 | 重建一次 authority，正文登记为 `legacy_candidate`，生命周期为 `imported_unverified`，只过 typed hard gate 后进入统一 settlement/approval。 |
| 旧已批准章节 | 不改正文、不重审、不重生成 execution package；legacy receipt 只证明历史状态，不伪造旧时不存在的 typed audit。 |
| 低层 action | add/revise/import 不能直接写 approved；调用方 passed verdict 不能替代本地 receipt；`update_truth` 只允许显式 administrative override、原因与准确 cutoff。 |
| rolling batch | 每批释放并重新获取唯一 lease，从磁盘连续 approval receipt 重算进度；durable goal 保存目标、档位、authority、next chapter、pause/cancel 状态。 |
| provider 恢复 | 通用网关对 provider-disconnect pause 做有界健康探测，服务恢复后调用现有 rescheduler；用户明确 pause/cancel 永不自动覆盖。 |
| typed completion | 目标 approved units、终局 typed evidence、must-resolve/payoff、hook overdue debt 与最终 receipt/truth 一致共同决定 complete；结尾关键词不参与 hard gate。 |
| 删除清单 | 旧恢复重规划、可直接 approved 的低层写入、普通自动 `update_truth`、`DefaultHasher` durable identity、自然语言 completion hard gate、一次规划全部 40～200 章。 |
| 验证证据 | legacy migration、伪造 review/status、administrative override、40/200 容量、batch checkpoint、provider pause 与 user pause 区分、typed completion 测试。 |

结论：代码路径已对齐。真实整本压力测试按原计划在 Phase 1～5 的静态、单元和故障验证全部通过后单独进行。

## 8. 重复与冲突机制删除审计

完成时必须由 `rg` 证明以下旧 owner 不再有生产定义或调用：

- `MAX_CREATION_CONTRACT_AUTO_REPAIR_ATTEMPTS`
- `MAX_CHAPTER_REVISION_ATTEMPTS`
- `audit_issue_is_actionable`
- `audit_issue_has_hard_blocking_marker`
- `audit_has_only_non_actionable_issues`
- `revise_reusable_existing_chapter_once`
- `sync_pending_settlement_metadata`
- durable identity 中的 `DefaultHasher`
- completion 的结尾/新阶段关键词 hard gate
- 恢复路径重新生成 execution package

兼容 serde 字段和 legacy parser 可以保留，但必须只读、迁移后立即转为 canonical 新结构，不能再次参与新写入权威。

## 9. 二次逐项核对中发现并完成的缺口

首次实施后没有直接把“代码看起来具备能力”当作完成。按最终完成定义和故障矩阵反向
核对后，又发现并修正了以下缺口：

1. `ContextSource` 在调用方已经完成分层选择后又统一截断 1200 字符，导致受保护合同、
   Story Bible、truth、计划和架构可能在封存前被静默截断。现已删除这层重复截断；可
   压缩来源仍由各自选择器控制预算，并增加 protected trace 等长断言。
2. 跨章节完全重复正文曾受已保存 Markdown 标题影响，无法稳定识别。现统一通过正文
   record normalizer 后比较；完全相同正文是 typed deterministic hard blocker，相似度
   仍只作 advisory。
3. 元数据修复测试原来检查的是已经弃用的 legacy settlement 路径，不能证明正确不变量。
   现改为读取 canonical pending settlement，验证正文不变时 state delta、hook delta、
   body/authority fingerprint 原样保留。
4. 10 万/2500 与 100 万/5000 只有规划代码，没有明确的 40/200 章 checkpoint 容量
   回归。现增加逐 rolling batch 写 checkpoint、模拟进程重建并从 durable next chapter
   恢复的容量测试，确认分别规划 40 与 200 章，单批始终不超过现有 3 章上限。
5. approval 在“receipt 已写、journal 仍为 prepared”处崩溃时，重放会返回成功但不会
   收口 journal。现由原 `approval_transaction` owner 校验 transaction/body/authority
   一致后把 journal 原子推进为 committed；没有新增第二套事务。故障测试覆盖提交前
   回滚、manifest 已提交后补 receipt、receipt 已写后补 journal，以及最终幂等重放。
6. 部分旧测试依赖空 observer 输出自动推导状态、缺失 accepted best 或弱化后的审批
   顺序。夹具已统一改为 sealed authority → accepted best → typed audit → final-body
   settlement → approval，不再通过测试旁路保留旧机制。

## 10. 最终完成定义 26 项复核

| # | 结果 | 当前唯一证据/owner |
| --- | --- | --- |
| 1 | 通过 | `ContractModelPatchBudget` 默认 1 次，净提升才允许第 2 次，绝对上限 2；30 轮符号已删除。 |
| 2 | 通过 | semantic `Uncertain` 只记录 advisory，不重开结构完整合同。 |
| 3 | 通过 | 合同候选以 typed field strength、用户权威和净提升向量比较。 |
| 4 | 通过 | LLM score 只进入 telemetry/advisory，不能产生 blocking finding。 |
| 5 | 通过 | 合同、连续性和状态冲突必须有 authority/body 的可验证证据，确定冲突仍 hard block。 |
| 6 | 通过 | 修订预算耗尽保留 best pending；未消除 hard blocker 不能批准。 |
| 7 | 通过 | 四个 role projection 都绑定同一 `authority_root_fingerprint`。 |
| 8 | 通过 | projection 有 coverage/included/excluded/truncated trace；protected source 不再二次截断。 |
| 9 | 通过 | sealed root 保存 canonical contract 与 as-of truth；兼容镜像不进入 role 权威。 |
| 10 | 通过 | 新写、恢复、legacy candidate、step retry 与 regenerate 进入同一 revision controller。 |
| 11 | 通过 | canonical `chapter-N.best.json` 保存完整 DraftOutput、typed findings、质量向量及全部依赖指纹。 |
| 12 | 通过 | required outcome、保护事实、锚点损失和 material deletion 参与净提升判定。 |
| 13 | 通过 | Writer 主协议是 title/body 长文本流；metadata 后置修复，旧 JSON 仅兼容读取。 |
| 14 | 通过 | Story Bible durable reducer 只消费 approved typed delta。 |
| 15 | 通过 | 人物、世界、关系和伏笔重大变化同时校验 final-body exact evidence 与 chapter contract allowance。 |
| 16 | 通过 | observer/settlement 失败进入 `state_repair_required`，pending 失败不会提交旧 truth，下一章受生命周期阻断。 |
| 17 | 通过 | receipt 在 settlement、truth 与最终 metadata 投影后生成；同正文 metadata repair 保留 settlement 事实。 |
| 18 | 通过 | before image + prepared/committed journal 覆盖所有写入间崩溃点，重放不重复应用 truth。 |
| 19 | 通过 | add/revise/import/review/update_truth 均不能绕过 canonical approval。 |
| 20 | 通过 | 第 8 节旧符号无生产引用；自然语言 metadata reducer 与重复修订 owner 已删除。 |
| 21 | 通过 | 2500/5000 档 hard max 分别为 5000/10000；总目标保持任意正整数。 |
| 22 | 通过 | durable goal、sealed authority、best、receipt、truth 和显式 pause/cancel 都可跨重启读取。 |
| 23 | 通过 | legacy authority/truth 按 chapter cutoff 重建，测试覆盖未来事实不能泄漏到早章。 |
| 24 | 通过 | 10 万/2500 精确规划 40 章，以有限 rolling batch 和磁盘连续 approval 进度推进。 |
| 25 | 通过 | 100 万/5000 精确规划 200 章，并通过所有 batch 边界 checkpoint/recovery 容量测试。 |
| 26 | 通过 | `complete` 只读 approved units、typed completion debts、hook/payoff 与 receipt/truth；结尾词只用于用户意图或 advisory。 |

结论：前置 A 与 Phase 1～5 的生产调用链、删除项和自动化验收已经对齐。Phase 6 未
实施。这里的“通过”指代码、单元/集成和故障注入层；不同本地模型下完整写完三本小说
仍属于下一轮真实聊天压力验收，不能用自动化夹具替代，也不应反向写入题材特例。

## 11. 最终验收命令

```text
cargo fmt --all -- --check
cargo check -p benshu-builtin-tools
cargo check -p benshu-gateway
cargo test -p benshu-builtin-tools writing
cargo test -p benshu-gateway provider_recovery_never_overrides_a_later_explicit_user_pause
rg <第 8 节弃用符号>
git diff --check
```

只有上述检查全部通过，才能把前置 A、Phase 1～5 标记为完全完成。Phase 6 不在本次验收范围。
