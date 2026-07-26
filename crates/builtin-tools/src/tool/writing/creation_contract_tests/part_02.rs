    #[test]
    fn creation_contract_rejects_and_repairs_goal_without_chapter_title() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-title-missing",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let mut contract = format!(
            "### 标准小说合同草案\n\
* **书名**：轮钥归途\n\
* **语言**：zh-CN\n\
* **题材**：异世界重生玄幻\n\
* **总字数**：50000\n\
* **每章目标档位**：2500\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：底层修士与资源垄断者的冲突。\n\
* **结局承诺**：主角重塑规则。\n\
* **世界观意象**：轮钥、阶梯城和重塑后的规则碑。\n\
* **总主线因果链**：轮钥觉醒引出垄断秩序，背叛和试炼推动终局重塑规则。\n\
* **命名理由**：书名来自主角用轮钥打开归途并重塑规则的结局。\n"
        );
        let titles = [
            "寒鸦鸣",
            "灵核残片",
            "黑岩镇",
            "夺取秘宝",
            "代价交换",
            "迷雾林",
            "风暴心",
            "抉择夜",
            "旧盟裂",
            "暗门开",
            "赤炉火",
            "龙骨桥",
            "",
            "天轨裂痕",
            "决战前奏",
            "秩序崩塌",
            "孤注一掷",
            "重塑时刻",
            "新纪元",
            "归途灯",
        ];
        for (offset, title) in titles.iter().enumerate() {
            let index = offset + 1;
            if index == 13 {
                contract.push_str("第13章：本章目标：遭遇背叛，主角在绝境中完成蜕变。\n");
            } else {
                contract.push_str(&format!(
                    "第{index:02}章《{title}》：本章目标：推进阶段事件并留下状态变化。\n"
                ));
            }
        }

        let issues = super::super::generated_contract_quality_issues(&draft, &contract);
        assert!(issues
            .iter()
            .any(|issue| issue.contains("逐章规划缺少章节名")));

        assert!(
            super::super::repair_creation_contract_plan_titles(&draft, &contract).is_none(),
            "local repair must not invent missing chapter titles from goal text"
        );
    }

    #[test]
    fn creation_contract_rejects_and_repairs_sentence_fragment_titles() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-title-fragment",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* **书名**：重塑天轨\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：个体自由与世界秩序的冲突。\n\
* **结局承诺**：主角建立新秩序。\n\
* **每章目标档位**：2500字\n\
第01章《破裂灵石》：本章目标：主角重生于矿区，意识到规则漏洞。\n\
第02章《过低级技》：本章目标：主角通过低级技巧获取第一份修为。\n\
第03章《夺命符咒》：本章目标：主角遭遇截杀，被迫选择禁忌路径。\n\
但不幸的代价：本章目标：主角通过献祭记忆换取力量。\n\
第04章《幽谷密谈》：本章目标：主角结识关键盟友，确认反派动向。\n\
第05章《角第一次》：本章目标：主角第一次直面审判官的投影。\n\
第06章《重塑骨骼》：本章目标：主角完成身体改造，脱离凡人范畴。\n\
第07章《风暴奏》：本章目标：主角逃离矿区，进入更高阶的城市。\n\
第08章《冰山一角》：本章目标：主角进入核心区域，发现世界真相。\n\
第09章《隐藏身份》：本章目标：主角在权贵圈层中获取核心资源。\n\
第10章《雷鸣战》：本章目标：主角身份暴露，引发大规模冲突。\n\
第11章《破碎契约》：本章目标：盟友背叛，主角陷入绝境。\n\
第12章《禁忌书》：本章目标：主角找到改写规则的媒介。\n\
第13章《审判时刻》：本章目标：主角直面审判官，展开正面交锋。\n\
第14章《角意识到》：本章目标：主角意识到改写规则需要巨大的牺牲。\n\
第15章《天轨裂痕》：本章目标：主角开始冲击规则核心。\n\
第16章《万象归一》：本章目标：世界秩序开始动摇，混乱蔓延。\n\
第17章《抉择巅》：本章目标：主角面临最终的生存或拯救选择。\n\
第18章《逆流而》：本章目标：主角挑战审判官的意志。\n\
第19章《规则重组》：本章目标：规则重组，旧秩序坍塌。\n\
第20章《尘埃落定》：本章目标：主角完成逆袭，建立新秩序。"
        );

        let issues = super::super::generated_contract_quality_issues(&draft, &contract);
        assert!(issues
            .iter()
            .any(|issue| issue.contains("逐章规划包含未编号目标行")));
        let advisory = super::super::generated_contract_advisory_issues(&draft, &contract);
        assert!(advisory
            .iter()
            .any(|issue| issue.contains("章节标题像句子残片")));

        // Fragment-like chapter titles are advisory at contract time; they are
        // repaired as chapter metadata after the body exists.
    }

    #[test]
    fn creation_contract_rejects_abstract_process_titles_from_real_contracts() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-abstract-title",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* **书名**：重塑天枢\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：底层拾荒者与世界规则的冲突。\n\
* **结局承诺**：主角重塑规则并付出代价。\n\
* **每章目标档位**：2500字\n\
第01章《寒铁矿脉》：本章目标：主角在矿脉底层意外觉醒前世记忆，意识到生存现状的残酷。\n\
第02章《锈蚀灵核》：本章目标：主角发现身体残缺的真相，并获得第一件关键物件。\n\
第03章《雷鸣重镇》：本章目标：主角进入城镇，通过交易换取初步的生存资源。\n\
第04章《秩序审判》：本章目标：主角第一次直面执法者，意识到规则对弱者的压榨。\n\
第05章《尝试第一》：本章目标：主角尝试第一次违背规则，导致身体遭受反噬。\n\
第06章《迷雾森林》：本章目标：主角在逃亡中进入未知区域，发现法则碎片。\n\
第07章《在生死关》：本章目标：主角在生死关头选择了牺牲部分力量来换取情报。\n\
第08章《圣女降临》：本章目标：主角与核心反派阵营的代表首次接触。\n\
第09章《破碎契约》：本章目标：主角意识到盟友的背叛，转入地下活动。\n\
第10章《天枢祭坛》：本章目标：主角抵达关键地点，揭开世界规则的真相。\n\
第11章《法则冲突》：本章目标：主角尝试通过祭坛进行第一次规则修正。\n\
第12章《被迫在情感与力量》：本章目标：主角面对反噬，被迫在情感与力量间做出选择。\n\
第13章《极北荒原》：本章目标：主角在寻找补完碎片的路径中遭遇自然灾害。\n\
第14章《旧日遗物》：本章目标：主角通过物件找回前世的关键记忆。\n\
第15章《秩序崩塌》：本章目标：世界规则出现裂痕，主角被迫卷入大规模冲突。\n\
第16章《决战夜》：本章目标：主角集结所有资源，准备进行最终的修正。\n\
第17章《天枢重塑》：本章目标：主角通过献祭实现规则的彻底转变。\n\
第18章《秩序巅》：本章目标：主角面对世界意志的最后阻拦。\n\
第19章《冲突解决》：本章目标：冲突解决，世界进入新的平衡态。\n\
第20章《成长弧线》：本章目标：主角回归平凡，完成最终的成长弧线。"
        );

        let issues = super::super::generated_contract_quality_issues(&draft, &contract);
        assert!(!issues
            .iter()
            .any(|issue| issue.contains("章节标题像句子残片")));
        let advisory = super::super::generated_contract_advisory_issues(&draft, &contract);
        assert!(advisory
            .iter()
            .any(|issue| issue.contains("章节标题像句子残片")));
        assert!(
            super::super::repair_creation_contract_plan_titles(&draft, &contract).is_none(),
            "abstract process titles should stay advisory until chapter metadata repair"
        );
    }

    #[test]
    fn creation_contract_rejects_chopped_candidate_titles_after_local_repair() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-chopped-title",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 小说创作合同草案\n\
* **书名**：重铸苍穹\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：个体意志与世界法则的冲突。\n\
* **结尾承诺**：主角献祭力量，重塑规则。\n\
* **每章目标档位**：2500字\n\
第01章《寒潭惊变》：本章目标：主角重生于寒潭，意外融合残缺灵核。\n\
第02章《破损灵脉》：本章目标：确认修为现状，决定通过特殊路径修复经脉。\n\
第03章《过交易获》：本章目标：通过交易获取第一件关键物件。\n\
第04章《展现天赋》：本章目标：展现天赋，引发门派关注与误解。\n\
第05章《枯萎灵草》：本章目标：寻找资源，意识到世界规则的剥夺性。\n\
第06章《重逢旧识》：本章目标：与前世关联人物重逢，建立情感纽带。\n\
第07章《迷雾森林》：本章目标：遭遇第一次法则冲突，被迫学会战斗技巧。\n\
第08章《夺命符咒》：本章目标：遭遇伏击，意识到反派势力的影子。\n\
第09章《秘境入口》：本章目标：获取进入高级资源区的资格。\n\
第10章《次直面世》：本章目标：第一次直面世界规则的恶意。\n\
第11章《断剑重铸》：本章目标：通过代价换取力量的提升。\n\
第12章《冲突升级》：本章注：本章目标：冲突升级，主角身份面临暴露风险。\n\
第13章《真相碎片》：本章目标：发现世界运行的终极逻辑。\n\
第14章《信任危机》：本章目标：遭遇信任危机，被迫做出艰难抉择。\n\
第15章《关键物件》：本章目标：寻找对抗规则的关键物件。\n\
第16章《天枢觉醒》：本章目标：面对法则意志的第一次正面交锋。\n\
第17章《心反派展》：本章目标：与核心反派展开对峙。\n\
第18章《灵魂博弈》：本章目标：在毁灭与重塑之间寻找第三条路。\n\
第19章《苍穹重塑》：本章目标：献祭力量，完成世界规则的微调。\n\
第20章《黎明曙光》：本章目标：尘埃落定，主角走向新的秩序。"
        );

        let issues = super::super::generated_contract_quality_issues(&draft, &contract);
        assert!(!issues
            .iter()
            .any(|issue| issue.contains("章节标题像句子残片")));
        let advisory = super::super::generated_contract_advisory_issues(&draft, &contract);
        assert!(advisory
            .iter()
            .any(|issue| issue.contains("章节标题像句子残片")));

        assert!(
            super::super::repair_creation_contract_plan_titles(&draft, &contract).is_none(),
            "chopped titles should not be locally invented during contract intake"
        );
    }

    #[test]
    fn creation_contract_rejects_second_pass_chopped_and_meta_titles() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-second-pass-chopped-title",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* **书名**：重塑天律\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：个体情感的渴望与世界绝对秩序之间的冲突。\n\
* **结尾承诺**：主角打破绝对平衡，让世界进入自由的新纪元。\n\
* **每章目标档位**：2500字\n\
* **逐章规划**：\n\
来看\n\
第01章《重生在贫》：本章目标：主角重生在贫瘠小镇，发现身体异样并意识到规则的压迫。\n\
第02章《禁忌咒文》：本章目标：主角意外接触到违规力量，引发第一次天罚预警。\n\
第03章《逃离秩序哨岗》：本章目标：主角为了生存，被迫离开安稳的村落，踏上流亡之路。\n\
第04章《断裂灵脉》：本章目标：在荒野中遭遇灵力枯竭的危机，主角被迫寻找替代能源。\n\
第05章《重逢旧识》：本章目标：偶遇前世碎片，建立初步的情感羁绊。\n\
第06章《交换寿命》：本章目标：主角通过交换寿命换取了关键的进阶道具。\n\
第07章《识到世界》：本章目标：主角意识到世界真相的冰山一角，决定不再顺从。\n\
第08章《风暴集市》：本章目标：在混乱的贸易点中，主角通过智斗获取了关键情报。\n\
第09章《审判者》：本章目标：执法者出现，主角在生死边缘完成力量突破。\n\
第10章《规则混乱》：本章目标：主角夺取关键物件，引发区域性的规则混乱。\n\
第11章《迷雾抉择》：本章目标：主角面临情感与生存的冲突，做出痛苦的选择。\n\
第12章《无声抗争》：本章目标：主角开始在暗处建立自己的势力，挑战规则。\n\
第13章《血色祭坛》：本章目标：揭露反派的真实目的，冲突激化。\n\
第14章《绝境觉醒》：本章目标：主角能力达到临界点，准备迎接最终决战。\n\
第15章《冲入秩序》：本章目标：主角冲入秩序核心，开启最终阶段。\n\
第16章《秩序崩塌》：本章目标：大规模规则紊乱，世界进入混乱状态。\n\
第17章《意识与力》：本章目标：主角与核心反派进行意识与力量的双重较量。\n\
第18章《牺牲重量》：本章目标：主角面临是否彻底毁灭旧世界的艰难选择。\n\
第19章《重塑规则》：本章目标：旧秩序瓦解，主角通过重塑规则实现新生。\n\
第20章《结局定格》：本章目标：结局定格，世界回归自由但充满挑战的新常态。"
        );

        let sanitized = super::super::sanitize_generated_contract_surface(&draft, &contract);
        let issues = super::super::generated_contract_quality_issues(&draft, &sanitized);
        assert!(!issues
            .iter()
            .any(|issue| issue.contains("章节标题像句子残片")));
        let advisory = super::super::generated_contract_advisory_issues(&draft, &sanitized);
        assert!(advisory
            .iter()
            .any(|issue| issue.contains("章节标题像句子残片")));

        assert!(
            super::super::repair_creation_contract_plan_titles(&draft, &sanitized).is_none(),
            "second-pass chopped titles should stay advisory until chapter metadata repair"
        );
    }

    #[test]
    fn creation_contract_rejects_meta_action_titles_and_markdown_residue() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-meta-action-title",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* **书名**：重塑天律\n\
* **主角**：{protagonist}\n\
并**关系线**：主角与导师建立传承关系。\n\
* **核心矛盾**：凡人意志与天律压制之间的冲突。\n\
* **结尾承诺**：主角打破循环秩序，建立新律。\n\
* **每章目标档位**：2500字\n\
第01章《寒门残响》：本章目标：主角重生于落魄家族，发现体内残留的法则碎片。\n\
第02章《青石镇雨》：本章目标：主角通过收集资源，第一次尝试引导灵力。\n\
第03章《断裂剑意》：本章目标：主角在冲突中被迫展示力量，造成不可逆的后果。\n\
第04章《古庙残影》：本章目标：主角在遗迹中寻获关键物件，引发规则波动。\n\
第05章《修为提升》：本章目标：主角意识到修为提升带来的副作用，产生动摇。\n\
第06章《风暴奏》：本章目标：主角面对家族危机，做出第一个重大的生存选择。\n\
第07章《逃离界》：本章目标：主角被迫离开家乡，踏入更广阔的世界。\n\
第08章《极北寒流》：本章目标：主角在严酷环境中磨炼意志，寻找进阶之法。\n\
第09章《法则痕》：本章目标：主角发现世界规则的漏洞，开始接触核心秘密。\n\
第10章《迷雾重重》：本章目标：主角在势力纷争中陷入误会，被迫隐匿身份。\n\
第11章《重塑心窍》：本章目标：主角通过突破，初步掌握新的力量形态。\n\
第12章《秩序守卫》：本章目标：反派势力介入，主角遭遇第一次正面对抗。\n\
第13章《破碎盟约》：本章目标：主角发现盟友的背叛，面对情感与利益的选择。\n\
第14章《生死关头》：本章目标：主角在生死关头完成力量的质变。\n\
第15章《天律震荡》：本章目标：主角的行为引起世界规则的剧烈反弹。\n\
第16章《禁忌门》：本章目标：主角进入核心区域，揭开最终真相。\n\
第17章《拯救世界》：本章目标：主角必须在拯救世界与保留自我之间做出决断。\n\
第18章《秩序崩塌》：本章目标：主角挑战反派，引发世界格局的剧变。\n\
第19章《余波未平》：本章目标：战斗后的代价显现，世界进入混乱的新阶段。\n\
第20章《新律初现》：本章目标：主角确立新的秩序，完成最终的蜕变。"
        );

        let issues = super::super::generated_contract_quality_issues(&draft, &contract);
        assert!(
            issues.iter().any(|issue| issue.contains("异常列表前缀")),
            "{issues:?}"
        );
        let advisory = super::super::generated_contract_advisory_issues(&draft, &contract);
        assert!(
            advisory
                .iter()
                .any(|issue| issue.contains("章节标题像句子残片")),
            "{advisory:?}"
        );

        assert!(
            super::super::repair_creation_contract_plan_titles(&draft, &contract).is_none(),
            "meta action titles should not be replaced by local synthetic titles during intake"
        );
    }

    #[test]
    fn creation_contract_rejects_contract_field_as_book_title_and_grammar_shards() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-contract-field-title",
            "fiction",
            "异世界重生玄幻，主角草根逆袭，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* 书名：主题承诺\n\
* 主角：{protagonist}\n\
* 核心矛盾：底层个体与记忆代价规则之间的冲突。\n\
* 结尾承诺：主角以关键选择改写世界规则，完成草根逆袭。\n\
* 每章目标档位：2500字\n\
第01章《择获得关》：本章目标：主角在黑市中通过选择获得关键资源。\n\
第02章《临由于修》：本章目标：主角面临由于修为提升带来的认知危机。\n\
第03章《寒潭碎影》：本章目标：主角在寒潭中觉醒残缺记忆，获得第一件法器。\n\
第04章《破损灵纹》：本章目标：主角修复灵纹，初步感知世界规则。\n\
第05章《林间抉择》：本章目标：主角为救同伴，被迫支付部分情感代价。\n\
第06章《风暴前奏》：本章目标：主角遭遇小规模冲突，展现潜力。\n\
第07章《重拾感知》：本章目标：主角通过冲突意识到力量与记忆的关联。\n\
第08章《枯萎记忆》：本章目标：主角因过度使用力量导致记忆模糊。\n\
第09章《城池入场券》：本章目标：主角通过交换代价，进入更高阶的城市。\n\
第10章《错位重逢》：本章目标：主角与旧识在模糊记忆中偶遇。\n\
第11章《战斗强化》：本章目标：主角通过战斗强化法器。\n\
第12章《真相裂痕》：本章目标：主角发现世界观的第一个谎言。\n\
第13章《巅峰幻象》：本章目标：主角达到修为顶峰，世界开始崩塌。\n\
第14章《宿命对决》：本章目标：主角与对手进行关于规则的最终对决。\n\
第15章《破碎王冠》：本章目标：主角意识到权力的代价是彻底的孤独。\n\
第16章《最后抉择》：本章目标：主角面临留住力量还是留住自我的选择。\n\
第17章《记忆余温》：本章目标：主角通过献祭力量换回部分情感。\n\
第18章《规则崩塌》：本章目标：主角彻底斩断与异世界的联系。\n\
第19章《归途钟声》：本章目标：主角回望一路代价并准备最终选择。\n\
第20章《晨曦微光》：本章目标：主角回归平凡生活，完成结局。"
        );

        let issues = super::super::generated_contract_quality_issues(&draft, &contract);
        assert!(
            issues.iter().any(|issue| issue.contains("书名")
                && (issue.contains("合同字段") || issue.contains("命名理由"))),
            "{issues:?}"
        );
        let advisory = super::super::generated_contract_advisory_issues(&draft, &contract);
        assert!(
            advisory
                .iter()
                .any(|issue| issue.contains("章节标题像句子残片")),
            "{advisory:?}"
        );

        assert!(
            super::super::repair_creation_contract_plan_titles(&draft, &contract).is_none(),
            "book-title repair must not rewrite grammar-shard chapter titles during intake"
        );
    }

    #[test]
    fn creation_contract_does_not_treat_editing_help_as_chapter_plan() {
        let help = "您可以针对以上任何部分（书名、角色设定、大纲走向、章节目标）提出修改意见。您可以直接对我说：“把主角名字改成XX”或“把第二阶段的目标改为XX”。";
        assert!(
            super::super::malformed_goal_like_plan_line_issue(help).is_none(),
            "editing help should not be parsed as a malformed chapter row"
        );
        assert!(
            super::super::line_looks_like_malformed_chapter_plan_goal(
                "第0可章《野火燎原》：本章目标：主角通过冲突展现潜力。"
            ),
            "malformed chapter rows should still be detected"
        );
    }

    #[test]
    fn creation_draft_approval_respects_negation_and_deferred_start() {
        assert!(!super::super::creation_draft_approval_requested(
            "请继续完善世界观和长线大纲，不要开始写正文。"
        ));
        assert!(!super::super::creation_draft_approval_requested(
            "请按这个方向更新合同草案，书名和角色都要原创，剧情要完整，有清晰结尾。"
        ));
        assert!(!super::super::creation_draft_execution_requested(
            "请按这个方向更新合同草案，书名和角色都要原创，剧情要完整，有清晰结尾。",
            "fiction"
        ));
        assert!(!super::super::creation_draft_approval_requested(
            "书名可以由系统随机生成，但不要写正文。"
        ));
        assert!(!super::super::creation_draft_approval_requested(
            "先定合同、大纲、角色名字和结局，不要直接写正文。"
        ));
        assert!(!super::super::creation_draft_execution_requested(
            "先定合同、大纲、角色名字和结局，不要直接写正文。",
            "fiction"
        ));
        assert!(!super::super::creation_draft_approval_requested(
            "等我下一条明确说开始写后再进入正文。"
        ));
        assert!(super::super::creation_draft_approval_requested("开始写"));
        assert!(super::super::creation_draft_approval_requested(
            "现在启动正式写作，正文保存为txt"
        ));
        assert!(super::super::creation_draft_approval_requested(
            "这个创作合同确认。现在开始正式写作。"
        ));
        assert!(super::super::creation_draft_approval_requested(
            "请更新合同草案，并开始正式写作。"
        ));
        assert!(super::super::creation_draft_approval_requested("确认"));
    }

    #[test]
    fn creation_draft_you_decide_with_planning_content_is_not_approval() {
        assert!(!super::super::creation_draft_approval_requested(
            "主角你来定，普通人在大城市里成长，感情线慢热。"
        ));
        assert!(super::super::creation_draft_approval_requested(
            "可以，你来定"
        ));
    }

    #[test]
    fn stateful_creation_intent_uses_real_draft_status() {
        let intent = super::super::classify_creation_draft_turn_intent_with_context(
            "按这个开始，写第一章",
            true,
            Some(super::super::CreationDraftLifecycleStatus::ContractReady),
            None,
            None,
        );

        assert_eq!(
            intent,
            super::super::CreationDraftTurnIntent::ApproveAndStart
        );
    }

    #[test]
    fn contract_status_question_is_read_status_not_update() {
        let intent = super::super::classify_creation_draft_turn_intent_with_context(
            "合同生成好了吗？如果好了，请展示可确认合同；如果还没好，请说明还缺什么。",
            true,
            Some(super::super::CreationDraftLifecycleStatus::ContractReady),
            None,
            None,
        );

        assert_eq!(intent, super::super::CreationDraftTurnIntent::ReadStatus);
    }

    #[tokio::test]
    async fn approved_start_turn_routes_to_writer_instead_of_planning_again() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写10万字。",
        )
        .expect("draft");
        draft.title = "雨巷灵火".to_string();
        draft.genre = "都市玄幻".to_string();
        draft.target_units = Some(15000);
        draft.chapter_unit_target = Some(2500);
        draft.brief =
            "普通人在大城市里进入灵能夜校体系，靠考试、代价和慢热关系完成逆袭。".to_string();
        draft.fiction_premise =
            "许闻在雨夜发现城市灵能裂缝，进入旧楼夜校，通过试炼和考试逐步守住城市。".to_string();
        draft.fiction_themes = vec!["草根逆袭必须付出代价".to_string()];
        draft.fiction_characters = vec![
            "name: 许闻; role: 主角; desire: 改变命运; fear: 再次失去家人; bottom_line: 不牺牲无辜者; arc_start: 被动卷入; arc_end: 主动守城"
                .to_string(),
            "name: 商砚衡; role: 关键对手; desire: 维护夜校裂缝利益; fear: 真相公开; bottom_line: 不亲手毁掉夜校体系; arc_start: 幕后施压; arc_end: 被证据逼到台前"
                .to_string(),
        ];
        draft.fiction_world_rules = vec!["城市灵能裂缝会把考试失败者的记忆作为燃料。".to_string()];
        draft.fiction_style_rules = vec!["用场景、行动和对话推进，不写提纲式正文。".to_string()];
        draft.fiction_must_avoid = vec!["不要改名，不要把工具日志写入正文。".to_string()];
        draft.narration_contract.pov = "第三人称有限视角".to_string();
        draft.fiction_ending_direction = "许闻在终局关闭吞噬城市的灵能裂缝。".to_string();
        draft.fiction_protagonist_arc =
            "从被动卷入异常事件的普通人，成长为主动守住城市的人。".to_string();
        draft.fiction_world_imagery = "雨巷灵火、旧楼夜校、玻璃天台裂缝。".to_string();
        draft.fiction_main_causal_spine =
            "城市异常引出夜校试炼，失败代价逼近反派真相，终局守城。".to_string();
        draft.fiction_title_rationale =
            "雨巷取自第一章城市异常的入口场景，灵火取自许闻终局关闭裂缝并守住城市的关键力量。"
                .to_string();
        draft.fiction_outline =
            "第1卷《雨巷入局》：本卷目标：许闻进入夜校试炼并取得借灵证黑幕证据；卷尾变化：许闻确认地下灵轨仍在扩大并决定继续追查。\n\
第2卷《裂缝晨光》：本卷目标：许闻公开借灵证证据、进入裂缝核心并关闭吞噬城市的灵能裂缝；卷尾变化：许闻关闭吞噬城市的灵能裂缝并公开夜校规则。\n\
第01章《雨巷灵火》：本章目标：主角发现城市灵能异常并做出第一次选择；预期转折：主角接受夜校试炼邀请，失去旁观退路。\n\
第02章《旧楼试炼》：本章目标：主角付出代价获得入局资格；预期转折：试炼代价留下无法撤销的记忆损耗。\n\
第03章《玻璃天台》：本章目标：主角识破反派线索；预期转折：对手察觉追查并封锁证据，地下灵轨的来源仍未解决。"
                .to_string();
        assert!(super::super::rebuild_current_contract_from_visible_draft(
            &mut draft
        ));
        draft.refresh_contract_status_from_validation();
        assert!(
            super::super::creation_draft_contract_blocking_issues(&draft).is_empty(),
            "approved start fixture must be contract-ready: {:?}",
            super::super::creation_draft_contract_blocking_issues(&draft)
        );

        let mut runtime = MockCreationDraftRuntime {
            draft: Some(draft),
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/test-project".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome =
            super::super::handle_creation_draft_chat(&mut runtime, "session-a", "开始写第一章")
                .await
                .expect("handled")
                .expect("outcome");
        let prompt = match outcome {
            super::super::CreationDraftTurnOutcome::ContinueWithMessage(prompt) => prompt,
            super::super::CreationDraftTurnOutcome::Respond(response) => {
                let persisted = runtime.draft.as_ref().expect("persisted draft");
                panic!(
                    "start turn should continue to writer, response was: {}; issues: {:?}; outline: {:?}; authority: {}",
                    response.response,
                    super::super::creation_draft_contract_blocking_issues(persisted),
                    persisted.fiction_outline,
                    persisted.current_contract.is_some()
                )
            }
        };
        assert!(prompt.contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER));
        assert!(prompt.contains("novel_studio"));
        assert!(prompt.contains("本轮范围：用户本轮只要求先写第一章"));
        assert!(!prompt.contains(super::super::CREATION_PLANNING_DIALOGUE_MARKER));
    }

    #[tokio::test]
    async fn explicit_start_with_incomplete_contract_blocks_instead_of_replanning() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-incomplete",
            "fiction",
            "都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.title.clear();
        draft.fiction_characters.clear();
        draft.fiction_outline.clear();

        let mut runtime = MockCreationDraftRuntime {
            draft: Some(draft),
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/test-project".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome =
            super::super::handle_creation_draft_chat(&mut runtime, "session-a", "开始写第一章")
                .await
                .expect("handled")
                .expect("outcome");
        let super::super::CreationDraftTurnOutcome::Respond(response) = outcome else {
            panic!("incomplete explicit start should be blocked, not replanned");
        };

        assert_eq!(runtime.approved, 0);
        assert!(response.response.contains("当前写作合同还不能进入正文写作"));
        assert!(response.response.contains("需要补齐"));
        assert!(!response
            .response
            .contains(super::super::CREATION_PLANNING_DIALOGUE_MARKER));
        assert!(!response
            .response
            .contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER));
    }

    #[test]
    fn planning_dialogue_does_not_require_no_prose_phrase() {
        assert!(super::super::creation_draft_planning_dialogue_requested(
            "写都市玄幻小说，每章2500字，至少5万字起。先和我多轮对话定大纲、主要情节和结局。"
        ));
        assert!(!super::super::creation_draft_approval_requested(
            "写都市玄幻小说，每章2500字，至少5万字起。先和我多轮对话定大纲、主要情节和结局。"
        ));
        assert!(super::super::creation_draft_approval_requested(
            "按这个开始写第一章"
        ));
        assert!(super::super::creation_draft_approval_requested(
            "按这个创作合同开始，先写第一章"
        ));
    }

    #[test]
    fn creation_draft_tool_args_pass_fiction_contract_fields_as_schema_fields() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写10万字。",
        )
        .expect("draft");
        draft.fiction_ending_direction = "许闻关闭吞噬城市的灵能裂缝。".to_string();
        draft.fiction_protagonist_arc =
            "从被动卷入异常事件的普通人，成长为主动守住城市的人。".to_string();
        draft.fiction_world_imagery = "雨巷灵火、旧楼夜校、玻璃天台裂缝。".to_string();
        draft.fiction_main_causal_spine =
            "城市异常引出夜校试炼，失败代价逼近反派真相，终局守城。".to_string();
        draft.fiction_title_rationale =
            "霓虹取自城市夜色意象，灵契取自许闻终局与城市灵能裂缝重新立约的核心选择。".to_string();

        let args = super::super::creation_draft_tool_args("approve", &draft);

        assert_eq!(args["action"], "approve_draft");
        assert_eq!(args["ending_direction"], draft.fiction_ending_direction);
        assert_eq!(args["protagonist_arc"], draft.fiction_protagonist_arc);
        assert_eq!(args["world_imagery"], draft.fiction_world_imagery);
        assert_eq!(args["main_causal_spine"], draft.fiction_main_causal_spine);
        assert_eq!(args["title_rationale"], draft.fiction_title_rationale);
    }

    #[test]
    fn creation_draft_blocks_incomplete_contract_and_generic_title_rationale() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写10万字。",
        )
        .expect("draft");
        draft.title = "霓虹余烬".to_string();
        draft.fiction_premise = "灵能考试改变城市阶层。".to_string();
        draft.fiction_characters =
            vec!["主角：许闻，欲望是改变命运，恐惧是再次失去家人。".to_string()];
        draft.fiction_ending_direction = "许闻公开灵能考试黑幕并重写城市晋级规则。".to_string();
        draft.fiction_protagonist_arc = "从旁听生变成愿意承担代价的规则改写者。".to_string();
        draft.fiction_world_imagery = "霓虹校门、灵能考场、旧城区余烬。".to_string();
        draft.fiction_main_causal_spine =
            "一次失败考试引出城市晋级黑幕，许闻逐步逼近幕后对手。".to_string();
        draft.fiction_title_rationale = "书名来自霓虹和余烬意象，体现作品气质。".to_string();
        draft.fiction_outline = "第01章《校门雨线》：本章目标：许闻进入灵能考场。".to_string();

        let issues = super::super::creation_draft_approval_readiness_issues(&draft);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("书名") && issue.contains("命名理由")),
            "{issues:?}"
        );

        draft.title = "校门焚榜人".to_string();
        draft.fiction_title_rationale =
            "校门取自许闻第一次闯入灵能考场的入口，焚榜指终局公开考试黑幕后烧毁旧晋级榜单的爽点兑现，人指许闻从旁听生成长为改写规则的人。"
                .to_string();
        let issues = super::super::creation_draft_approval_readiness_issues(&draft);
        assert!(!issues.iter().any(|issue| issue.contains("命名理由")), "{issues:?}");
    }

    #[test]
    fn creation_draft_control_only_message_does_not_pollute_brief() {
        let draft =
            super::super::build_initial_creation_draft("session-a", "fiction", "停止当前任务")
                .expect("draft");
        assert!(draft.brief.is_empty());

        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写10万字。",
        )
        .expect("draft");
        super::super::apply_message_to_creation_draft(&mut draft, "停止当前任务");
        assert!(!draft.brief.contains("停止当前任务"));

        draft.brief = "都市玄幻；停止当前任务；要有清晰结尾".to_string();
        let args = super::super::creation_draft_tool_args("approve", &draft);
        assert_eq!(args["brief"], "都市玄幻；要有清晰结尾");
    }

    #[tokio::test]
    async fn creation_draft_rejects_incomplete_completed_contract_when_active_draft_is_missing() {
        let mut runtime = MockCreationDraftRuntime {
            draft: None,
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/recovered-project".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome =
            super::super::handle_creation_draft_chat(&mut runtime, "session-a", "开始写第一章")
                .await
                .expect("handled");

        assert_eq!(runtime.approved, 0);
        assert!(outcome.is_none());
    }

    #[test]
    fn generated_contract_sanitizer_removes_assistant_surface_noise() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写10万字。",
        )
        .expect("draft");
        let noisy = "下面是待确认的小说创作合同草案。我还没有开始写正文；你可以继续修改。\n\
当前标准小说合同草案：\n\
已确认合同摘要：由于您尚未提供具体故事题材，请先提供故事设定。\n\
书名：霓虹灵契\n\
主角：许闻，欲望是改变命运。\n\
第01章《雨巷灵火》：本章目标：主角发现城市灵能异常。\n\
如果已经可以，请回复“开始写”。";
        let sanitized = super::super::sanitize_generated_contract_surface(&draft, noisy);
        assert!(!sanitized.contains("由于您尚未提供"));
        assert!(!sanitized.contains("回复“开始写"));
        assert!(sanitized.contains("书名：霓虹灵契"));
        assert!(sanitized.contains("第01章"));
    }

    #[test]
    fn generated_contract_quality_rejects_malformed_names_and_plan_fragments() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写10万字。",
        )
        .expect("draft");
        draft.target_units = Some(100000);
        draft.chapter_unit_target = Some(2500);
        let bad = "书名：冲突点\n\
主角：秩序守吗\n\
核心矛盾：主角要对抗城市灵能垄断。\n\
结局：主角守住城市。\n\
共1频率：异常。\n\
第并解决：阶段性冲突加剧。";
        let issues = super::super::generated_contract_quality_issues(&draft, bad);
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("合同命名字段异常")),
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("章节规划编号格式异常")),
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("合同数字格式异常")),
            "{issues:?}"
        );
    }

    #[test]
    fn generated_contract_quality_allows_normal_zero_start_phrase() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市爽文小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50_000);
        draft.chapter_unit_target = Some(2_500);
        let contract = "书名：零点翻盘\n\
命名理由：零点来自主角财富从0开始的处境，翻盘来自终局反转。\n\
主角姓名：顾砚川，欲望：靠自己的判断夺回被平台夺走的机会，恐惧：再次被规则压回底层。\n\
核心矛盾：城市平台用信用分垄断上升通道，主角要利用规则漏洞完成逆袭。\n\
结局：顾砚川公开平台暗箱账本，夺回公司控制权并改变规则。\n\
世界观意象：雨夜高架、信用分屏幕、旧合同、零点账本。\n\
第01章《雨夜入局》：本章目标：顾砚川在低谷中发现旧合同漏洞。";
        let issues = super::super::generated_contract_quality_issues(&draft, contract);
        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("合同数字格式异常")),
            "{issues:?}"
        );
    }

    #[test]
    fn generated_contract_quality_allows_story_world_measure_numbers() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "蒸汽朋克海港悬疑小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50_000);
        draft.chapter_unit_target = Some(2_500);
        let contract = "书名：肺中齿轮\n\
命名理由：肺中齿轮来自尸检线索，也是城市核心吞噬生命的证据。\n\
主角姓名：景予宁，欲望：偿还债务并保住机械心脏，恐惧：沦为锅炉房燃料。\n\
核心矛盾：海港依靠锅炉房抽取灵魂维持运转，景予宁必须揭开总督府骗局。\n\
结局：景予宁用自己的机械心脏替换城市核心，切断灵魂汲取。\n\
世界规则：每抽取1单位纯净灵魂，锅炉房必须消耗对应寿命债务；债务会在齿轮肺中留下可追溯痕迹；拒绝献祭者会被总督府伪造成无死因尸体。\n\
第01章《雾港尸检》：本章目标：景予宁在总督尸体肺部发现微型齿轮。";
        let issues = super::super::generated_contract_quality_issues(&draft, contract);
        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("合同数字格式异常")),
            "{issues:?}"
        );
    }

    #[test]
    fn generated_contract_quality_rejects_joined_contract_number_fields() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市爽文小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50_000);
        draft.chapter_unit_target = Some(2_500);
        let contract = "书名：账本翻盘\n\
命名理由：账本来自主角揭开平台暗箱的关键证据。\n\
主角姓名：顾砚川，欲望：夺回机会，恐惧：再次被规则压回底层。\n\
核心矛盾：平台用信用分垄断上升通道。\n\
结局：顾砚川公开平台暗箱账本并改变规则。\n\
总字数：5万字3每章2500字\n\
第01章《雨夜入局》：本章目标：顾砚川发现旧合同漏洞。";
        let issues = super::super::generated_contract_quality_issues(&draft, contract);
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("合同数字格式异常")),
            "{issues:?}"
        );
    }

    #[test]
    fn generated_contract_quality_does_not_require_marketing_hook_score() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写5万字。",
        )
        .expect("draft");
        draft.title = "缄默刻度".to_string();
        draft.target_units = Some(50_000);
        draft.chapter_unit_target = Some(2_500);
        let contract = "书名：缄默刻度\n\
命名理由：缄默代表主角失去记忆后的沉默，刻度代表能力消耗的计量。\n\
主角姓名：秦澈，欲望：通过学院考试改写底层命运，恐惧：记忆被城市评分系统吞掉。\n\
核心矛盾：折光城用灵能评分垄断晋级资格，主角必须揭开评分日志里的献祭真相。\n\
结局：秦澈在终局考试中公开评分日志，推翻折光城的记忆献祭制度。\n\
世界观意象：折光城、学院评分塔、记忆刻盘、灵能考场。\n\
第01章《折光入场》：本章目标：秦澈进入学院测试并发现评分异常。\n\
第02章《评分塔夜灯》：本章目标：秦澈追查隐藏日志。\n\
第03章《记忆刻盘》：本章目标：秦澈确认晋级制度吞噬失败者记忆。";

        let issues = super::super::generated_contract_quality_issues(&draft, contract);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("抽象概念") || issue.contains("读者钩子")),
            "{issues:?}"
        );
    }

    #[test]
    fn approved_creation_draft_prompts_formal_writer_execution() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "帮我写一个草根逆袭的玄幻小说，50万字",
        )
        .expect("draft");
        draft.draft_path = "data/generated/novels/drafts/draft.json".to_string();
        draft.chapter_unit_target = Some(5000);
        draft.max_chapters_per_turn = Some(1);
        let prompt = super::super::final_prompt_from_approved_creation_draft(
            &draft,
            &json!({"success": true, "init": {"project_path": "data/generated/novels/test-project"}}),
            "开始写",
        );

        assert!(prompt.contains("novel_studio"));
        assert!(prompt.contains("data/generated/novels/test-project"));
        assert!(prompt.contains("500000") || prompt.contains("500,000") || prompt.contains("50万"));
        assert!(prompt.contains("不要继续追问"));
    }

    #[test]
    fn approved_creation_draft_respects_first_chapter_turn_scope() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市言情小说，每章3000字，写10万字。",
        )
        .expect("draft");
        draft.target_units = Some(100000);
        draft.chapter_unit_target = Some(2500);
        let prompt = super::super::final_prompt_from_approved_creation_draft(
            &draft,
            &json!({"success": true, "init": {"project_path": "data/generated/novels/test-project"}}),
            "可以，按这个开始写第一章。",
        );

        assert!(prompt.contains("总目标字数：100000"));
        assert!(prompt.contains("每轮最多章节：1"));
        assert!(prompt.contains("不要因为总目标字数存在而连续生成全书"));
    }

    #[test]
    fn approved_creation_draft_first_chapter_beats_chat_display_full_text_constraint() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市言情小说，每章2500字，写5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let prompt = super::super::final_prompt_from_approved_creation_draft(
            &draft,
            &json!({"success": true, "init": {"project_path": "data/generated/novels/test-project"}}),
            "开始写第一章，正文保存成txt，不要把全文塞进聊天框。",
        );

        assert!(prompt.contains("每轮最多章节：1"));
        assert!(prompt.contains("本轮只要求先写第一章"));
        assert!(!prompt.contains("全部剩余章节"));
        assert!(!prompt.contains("直接生成完剩余内容"));
    }

    #[test]
    fn creation_draft_framework_negation_beats_all_book_and_chapter_count_terms() {
        let message = "书名选《雨夜后的微光》。请修正合同：全书和聊天回复都只用中文。先不要写正文，请给我最终版20章大纲和还需要确认的问题。";

        assert!(super::super::creation_draft_framework_requested(
            message, "fiction"
        ));
        assert!(!super::super::creation_draft_execution_requested(
            message, "fiction"
        ));
        assert!(!super::super::creation_draft_approval_requested(message));
    }

    #[test]
    fn approved_creation_draft_respects_explicit_chapter_turn_count() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "玄幻小说，每章2500字，先只写两章。",
        )
        .expect("draft");
        draft.target_units = Some(100000);
        draft.chapter_unit_target = Some(2500);

        assert_eq!(
            super::super::creation_draft_requested_turn_units(
                "可以，按这个开始写两章。",
                "fiction"
            ),
            Some(2)
        );
        assert_eq!(
            super::super::creation_draft_requested_turn_units(
                "可以，按这个开始写第2章。",
                "fiction"
            ),
            None
        );

        let prompt = super::super::final_prompt_from_approved_creation_draft(
            &draft,
            &json!({"success": true, "init": {"project_path": "data/generated/novels/test-project"}}),
            "可以，按这个开始写两章。",
        );

        assert!(prompt.contains("每轮最多章节：2"));
        assert!(prompt.contains("明确要求生成 2 章"));
        assert!(!prompt.contains("先完成第一章"));
    }

    #[test]
    fn creation_draft_turn_count_ignores_negated_small_batch_limit() {
        let message = "开始写。按已确认合同持续写完整本，直到完成约40章和10万字，不要只写三章。";

        assert_eq!(
            super::super::creation_draft_requested_turn_units(message, "fiction"),
            None
        );
        assert!(super::super::creation_draft_requests_all_remaining(
            message, "fiction"
        ));
    }

    #[test]
    fn approved_creation_draft_does_not_treat_prior_chapter_count_as_turn_count() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "10万字赛博朋克玄幻，每章5000字。",
        )
        .expect("draft");
        draft.status = "approved".to_string();
        draft.project_path = "data/generated/novels/test-project".to_string();
        draft.brief = "旧的续写第二章；继续第三章；从未通过的第五章接着修到合格".to_string();
        draft.planning_notes = vec![
            "继续写第二章，保持刚才的中文合同".to_string(),
            "主角需要保持陆远".to_string(),
        ];

        let message = "继续当前《问道纪》项目。从第15章开始接着写，保持已有故事合同、已批准前十四章、主角陆远、赛博朋克玄幻主线和中文写作。第15章之前需要修订，请先修好并通过审查，再继续写到全书约10万字并给出完整阶段收束。不要新建项目，不要重写前十四章。正文保存到文件，聊天只返回进度、章节号、字数、路径和审查状态。";

        assert_eq!(
            super::super::creation_draft_requested_turn_units(message, "fiction"),
            None
        );

        let mut continuation = draft.clone();
        super::super::apply_continuation_controls_to_creation_draft(&mut continuation, message);
        assert_eq!(continuation.brief, draft.brief);
        assert_eq!(continuation.planning_notes, draft.planning_notes);

        let prompt = super::super::final_prompt_from_approved_creation_draft(
            &continuation,
            &json!({"success": true, "project_path": "data/generated/novels/test-project"}),
            message,
        );

        assert!(!prompt.contains("明确要求生成 14 章"));
        assert!(!prompt.contains("旧的续写第二章"));
        assert!(prompt.contains("用户最新要求"));
        assert!(prompt.contains("以当前项目合同为准"));
    }

    #[test]
    fn approved_creation_draft_allows_followup_to_finish_remaining() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市言情小说，每章3000字，写10万字。",
        )
        .expect("draft");
        draft.status = "approved".to_string();
        draft.target_units = Some(100000);
        draft.chapter_unit_target = Some(2500);
        draft.max_chapters_per_turn = Some(1);

        assert!(super::super::creation_draft_execution_requested(
            "后面直接生成完",
            "fiction"
        ));
        assert!(!super::super::creation_draft_modification_requested(
            "后面直接生成完"
        ));

        let prompt = super::super::final_prompt_from_approved_creation_draft(
            &draft,
            &json!({"success": true, "init": {"project_path": "data/generated/novels/test-project"}}),
            "后面直接生成完",
        );

        assert!(prompt.contains("全部剩余章节"));
        assert!(prompt.contains("用户本轮要求直接生成完剩余内容"));
        assert!(!prompt.contains("每轮最多章节：1"));
    }

    #[test]
    fn approved_creation_draft_understands_composed_whole_artifact_scope() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "边境悬疑小说，每章2500字，写5万字。",
        )
        .expect("draft");
        draft.status = "approved".to_string();
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        draft.max_chapters_per_turn = Some(1);

        let prompt = super::super::final_prompt_from_approved_creation_draft(
            &draft,
            &json!({"success": true, "init": {"project_path": "data/generated/novels/test-project"}}),
            "按这个合同开始，请自动完成整部小说。",
        );

        assert!(prompt.contains("全部剩余章节"));
        assert!(prompt.contains("直接生成完剩余内容"));
        assert!(!prompt.contains("后续由用户继续"));
    }

    #[test]
    fn whole_artifact_scope_respects_negation_and_document_kind() {
        assert!(!super::super::creation_draft_requests_all_remaining(
            "先不要自动完成整部小说，只确认合同。",
            "fiction"
        ));
        assert!(super::super::creation_draft_requests_all_remaining(
            "请完成整篇报告。",
            "document"
        ));
    }

    #[test]
    fn approved_creation_draft_followup_can_change_chapter_band() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市言情小说，每章3000字，写10万字。",
        )
        .expect("draft");
        draft.status = "approved".to_string();

        assert!(super::super::creation_draft_modification_requested(
            "把后续每章改成5000字"
        ));
        super::super::apply_message_to_creation_draft(&mut draft, "把后续每章改成5000字");

        assert_eq!(draft.chapter_unit_target, Some(5000));
    }

    #[test]
    fn creation_draft_extracts_chinese_book_title_quotes() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写一部5万字的短篇爱情小说，每章2500字。",
        )
        .expect("draft");

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "选《雨夜后的微光》，女主沈汐，男主陆予。按这个开始写。",
        );

        assert_eq!(draft.title, "雨夜后的微光");
    }

    #[test]
    fn creation_draft_title_conflict_response_is_natural_language() {
        let response = super::super::creation_draft_approval_failure_response(&json!({
            "success": false,
            "error": "title_conflict",
            "title": "雨夜后的微光",
            "title_conflicts": [{
                "title": "雨夜后的微光",
                "path": "/tmp/novels/雨夜后的微光",
                "similarity": 1.0
            }]
        }));

        assert!(response.contains("标题《雨夜后的微光》已经存在"));
        assert!(response.contains("继续已有项目"));
        assert!(!response.contains("\"error\""));
    }

    #[test]
    fn creation_draft_detects_generated_title_revision_without_literal_title() {
        let message =
            "请根据刚才的大纲和结局重新取一个不同的新书名，主角不要叫陆离，其他合同保持不变";

        assert!(super::super::creation_draft_requests_generated_title_revision(message));
        assert!(super::super::creation_draft_modification_requested(message));
        assert!(
            !super::super::creation_draft_requests_generated_title_revision(
                "书名叫《星门余火》，按这个开始"
            )
        );

        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.title = "尘阶逆命录".to_string();
        super::super::apply_message_to_creation_draft(&mut draft, message);
        assert_eq!(draft.title, "尘阶逆命录");
        assert!(draft
            .planning_notes
            .iter()
            .any(|note| note == "失败合同禁用书名：尘阶逆命录"));
        assert!(draft
            .planning_notes
            .iter()
            .any(|note| note == "失败合同禁用角色名：陆离"));
        assert!(super::super::stable_creation_planning_notes(&draft)
            .iter()
            .all(|note| !note.contains("重新生成不同书名") && !note.contains("陆离")));
    }

    #[test]
    fn requested_title_does_not_treat_title_diagnostics_as_literal_title() {
        assert_eq!(
            super::super::requested_title("书名是否来自剧情和结局？小说名字是这个啊"),
            None
        );
        assert_eq!(
            super::super::requested_title("书名叫《星门余火》，按这个开始"),
            Some("星门余火".to_string())
        );
        assert_eq!(
            super::super::requested_title(
                "对手姓名“白澈白”首尾重复，请修正角色名，其他合同不要改"
            ),
            None
        );
        assert_eq!(
            super::super::requested_title("《星门余火》"),
            Some("星门余火".to_string())
        );
    }

    #[test]
    fn generated_contract_rejects_meta_discussion_as_book_title() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "书名：否来自剧情和结局\n\
命名理由：这个名字来自剧情和结局。\n\
角色权威表：主角姓名：许闻，欲望：通过灵考，恐惧：再次被抹去身份，底线：不牺牲同学。\n\
终局方向：许闻公开碑下证词，打破校盟配额。\n\
主角弧线：从旁听生到公开证词的人。\n\
世界观意象：碑下证词、灵考名册、雨夜校门。\n\
总主线因果链：旁听异常 -> 查证名册 -> 公开证词 -> 打破配额。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(
            issues.iter().any(|issue| issue.contains("元讨论")),
            "{issues:?}"
        );
    }

    #[test]
    fn creation_contract_counts_natural_chapter_plan_lines() {
        assert!(super::super::line_looks_like_explicit_chapter_plan(
            "然后第3章：第一缕星光 —— 觉醒残缺力量"
        ));
        assert!(super::super::line_looks_like_explicit_chapter_plan(
            "*   第 7章 试炼之难：进入秘境"
        ));
        assert!(super::super::line_looks_like_explicit_chapter_plan(
            "01. 《废墟中的微光》：本章目标：主角醒来并做出选择"
        ));
        assert!(super::super::line_looks_like_explicit_chapter_plan(
            "*   01章《微末之火》：本章目标：展示主角卑微现状。"
        ));
        assert!(!super::super::line_looks_like_explicit_chapter_plan(
            "*   第第14章：意志磨炼"
        ));
        assert!(!super::super::line_looks_like_explicit_chapter_plan(
            "1. 基本参数"
        ));
        assert!(!super::super::line_looks_like_explicit_chapter_plan(
            "2. 故事合同"
        ));
        assert!(super::super::chapter_plan_line_has_goal(
            "第08章《暗流涌动》：本章让目标：遭遇第一个反派，意识到世界的残酷规则。"
        ));
        assert_eq!(
            super::super::normalize_chapter_plan_goal_label(
                "第19章《重塑神魂》：本法目标：完成最终的蜕变，掌握新的力量逻辑。"
            ),
            "第19章《重塑神魂》：本章目标：完成最终的蜕变，掌握新的力量逻辑。"
        );
        assert_eq!(
            super::super::chapter_plan_title_from_line(
                "第02章《枯竭的脉络》：本章目标：寻找第一块记忆碎片。"
            )
            .as_deref(),
            Some("枯竭的脉络")
        );
        assert_eq!(
            super::super::chapter_plan_title_from_line(
                "*   01章《微末之火》：本章目标：展示主角卑微现状。"
            )
            .as_deref(),
            Some("微末之火")
        );
        assert_eq!(
            super::super::malformed_chapter_plan_fragment("*   第第14章：意志磨炼").as_deref(),
            Some("*   第第14章：意志磨炼")
        );
    }

    #[test]
    fn creation_contract_preserves_chapter_plan_lines_with_goal_colons() {
        let outline = super::super::generated_fiction_outline(
            "### 结构合同\n\
结尾承诺：主角重塑世界规则，完成最终救赎。\n\
第一卷：余烬觉醒\n\
第01章《废墟醒转》：本章目标：主角重生在废墟，意识到生命流逝的危机。\n\
第02章《黑炉试血》：本章目标：主角通过残缺印记获取第一缕火种。\n\
第03章《拾荒者困局》：本章目标：遭遇资源掠夺者，被迫进行第一次生死搏杀。\n\
第04章《火种代价》：本章目标：发现力量提升带来的副作用。\n",
        );

        assert_eq!(super::super::count_explicit_chapter_plan_lines(&outline), 4);
        assert!(outline.contains("第01章《废墟醒转》"));
        assert!(outline.contains("第04章《火种代价》"));
    }

    #[test]
    fn creation_contract_rejects_monotone_chapter_title_templates() {
        let text = r#"
第01章《寒门的微光》：本章目标：建立起点。
第02章《矿场的裂痕》：本章目标：推进冲突。
第03章《黑市的回声》：本章目标：推进冲突。
第04章《记忆的代价》：本章目标：推进冲突。
第05章《规则的审判》：本章目标：推进冲突。
第06章《荒原新火》：本章目标：转折。
第07章《夜市追逃》：本章目标：转折。
第08章《断桥重逢》：本章目标：收束。
"#;

        let issue = super::super::chapter_plan_title_diversity_issue(text, 8).unwrap();

        assert!(issue.contains("章节标题句式过于单一") || issue.contains("章节标题模板过于重复"));
    }
    #[test]
    fn ready_typed_contract_outline_reaches_novel_studio_tool_args() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-typed-outline",
            "fiction",
            "写科幻小说，每章2500字，总字数10万字。",
        )
        .expect("draft");
        draft.title = "潮汐遗嘱".to_string();
        draft.current_contract = Some(serde_json::json!({
            "title": {"canonical_title": "潮汐遗嘱"},
            "language": "zh-CN",
            "genre": "海洋科幻",
            "outline": {
                "raw_outline": "主角追查异常海流并在终局重置潮汐引擎。",
                "volumes": [{
                    "title": "深蓝失序",
                    "objective": "确认海流异常来自古代引擎",
                    "ending_change": "主角取得引擎控制密钥"
                }],
                "near_chapters": [{
                    "number": 1,
                    "goal": "收到来自海沟底部的规律脉冲",
                    "expected_turn": "脉冲与海平面上升同步"
                }]
            }
        }));

        let args = super::super::creation_draft_tool_args("draft", &draft);
        let outline = args["outline"].as_str().expect("outline");

        assert!(outline.contains("第1卷《深蓝失序》"), "{outline}");
        assert!(outline.contains("第1章 本章目标"), "{outline}");
        assert!(outline.contains("脉冲与海平面上升同步"), "{outline}");
    }
