use super::*;

impl DelegateTool {
    pub(crate) fn empty_sensory_hub() -> Arc<benshu_sensory::SensoryHub> {
        Arc::new(benshu_sensory::SensoryHub::new(
            benshu_sensory::SensoryConfig::default(),
        ))
    }

    pub(crate) async fn try_fast_path(
        &self,
        role: &AgentRole,
        blueprint_tools: &[String],
        task: &str,
    ) -> anyhow::Result<Option<String>> {
        let tool_set = blueprint_tools
            .iter()
            .map(|tool| tool.as_str())
            .collect::<std::collections::HashSet<_>>();

        if role.name() == "writer" && task.contains("[BENSHU_NOVEL_CONTENT_OPERATION]") {
            return self.try_novel_content_operation_fast_path(role, task).await;
        }

        if tool_set.contains("skill_manager") {
            let (Some(loader), Some(data_dir), Some(enabled_tools)) =
                (&self.skill_loader, &self.data_dir, &self.enabled_tools)
            else {
                return Ok(Some(
                    "status: blocked\nworker: skill_manager\nblockers: skill manager runtime dependencies are not attached to delegate".to_string(),
                ));
            };

            let manager = SkillManagerTool::new(
                loader.clone(),
                data_dir.clone(),
                enabled_tools.clone(),
                self.coordinator.clone(),
            );
            let source_url =
                Self::first_url(task).or_else(|| Self::extract_github_repo_shorthand(task));
            let skill_name = Self::extract_skill_name_for_management(task)
                .or_else(|| source_url.clone())
                .unwrap_or_else(|| task.trim().to_string());
            let lowered = task.to_ascii_lowercase();
            let inventory_requested = Self::is_skill_inventory_request(task);
            let explicit_hold = task.contains("不要安装")
                || task.contains("确认前")
                || task.contains("确认之前")
                || lowered.contains("do not install")
                || lowered.contains("before confirmation")
                || lowered.contains("before user confirmation");
            let explicit_confirm = task.contains("确认")
                || task.contains("继续安装")
                || lowered.contains("confirmed")
                || lowered.contains("user confirmed");
            let followup_after_install = task.contains("已安装")
                || lowered.contains(" is installed")
                || lowered.contains("has been installed")
                || (lowered.contains("installed") && lowered.contains("skill"));
            let confirmed_for_install = explicit_confirm || followup_after_install;
            let install_requested = !explicit_hold
                && (confirmed_for_install
                    || lowered.contains("install")
                    || task.contains("安装")
                    || task.contains("装备")
                    || task.contains("创建 worker"));

            let arguments = if inventory_requested {
                json!({
                    "action": "list",
                    "skill_name": skill_name
                })
            } else if install_requested {
                json!({
                    "action": "install",
                    "skill_name": skill_name,
                    "source_url": source_url,
                    "confirmed": confirmed_for_install || source_url.is_some()
                })
            } else {
                json!({
                    "action": "resolve",
                    "skill_name": source_url.unwrap_or(skill_name)
                })
            };
            let output = manager.call(&arguments.to_string()).await?;
            return Ok(Some(format!(
                "status: completed\nworker: skill_manager\nexecuted_tool: skill_manager\nresult:\n{}",
                output
            )));
        }

        if role.name() == "chart" && tool_set.contains("chart") {
            let Some(arguments) = Self::extract_chart_arguments(task) else {
                return Ok(Some(
                    "status: blocked\nworker: chart\nblockers: chart generation needs chart data, for example data={\"labels\":[...],\"values\":[...]}".to_string(),
                ));
            };
            let output = ChartTool::new("delegate-chart")
                .call(&arguments.to_string())
                .await?;
            return Ok(Some(format!(
                "status: completed\nworker: chart\nexecuted_tool: chart\nresult:\n{}",
                output
            )));
        }

        if role.name() == "repo" && tool_set.contains("git_ops") {
            let output = GitOpsTool
                .call(
                    &json!({
                        "action": "local_status",
                        "path": "."
                    })
                    .to_string(),
                )
                .await?;
            return Ok(Some(format!(
                "status: completed\nworker: repo\nexecuted_tool: git_ops\nresult:\n{}",
                output
            )));
        }

        if role.name() == "pdf" && tool_set.contains("pdf_parse") {
            let Some(path) = Self::extract_local_path(task) else {
                return Ok(Some(
                    "status: blocked\nworker: pdf\nblockers: pdf_parse needs a concrete local PDF path".to_string(),
                ));
            };
            let path = path.to_string_lossy().to_string();
            let output = PdfParseTool::new(None, None, Self::empty_sensory_hub())
                .call(
                    &json!({
                        "path": path,
                        "mode": "text",
                        "format": "markdown",
                        "image_output": "off",
                        "page_limit": 8
                    })
                    .to_string(),
                )
                .await?;
            return Ok(Some(format!(
                "status: completed\nworker: pdf\nexecuted_tool: pdf_parse\nresult:\n{}",
                output
            )));
        }

        if role.name() == "office" && tool_set.contains("office_parse") {
            let Some(path) = Self::extract_local_path(task) else {
                return Ok(Some(
                    "status: blocked\nworker: office\nblockers: office_parse needs a concrete local Office document path".to_string(),
                ));
            };
            let path = path.to_string_lossy().to_string();
            let output = OfficeParseTool
                .call(&json!({ "path": path }).to_string())
                .await?;
            return Ok(Some(format!(
                "status: completed\nworker: office\nexecuted_tool: office_parse\nresult:\n{}",
                output
            )));
        }

        if role.name() == "media" && tool_set.contains("probe_media") {
            let Some(path) = Self::extract_local_path(task) else {
                return Ok(Some(
                    "status: blocked\nworker: media\nblockers: probe_media needs a concrete local media path".to_string(),
                ));
            };
            let path = path.to_string_lossy().to_string();
            let output = ProbeMediaTool
                .call(&json!({ "path": path }).to_string())
                .await?;
            return Ok(Some(format!(
                "status: completed\nworker: media\nexecuted_tool: probe_media\nresult:\n{}",
                output
            )));
        }

        if role.name() == "ocr" && tool_set.contains("text_extract") {
            let Some(path) = Self::extract_local_path(task) else {
                return Ok(Some(
                    "status: blocked\nworker: ocr\nblockers: text_extract needs a concrete local image path".to_string(),
                ));
            };
            let path = path.to_string_lossy().to_string();
            let output = TextExtractTool::new(None, None, Self::empty_sensory_hub())
                .call(&json!({ "action": "recognize", "path": path }).to_string())
                .await?;
            return Ok(Some(format!(
                "status: completed\nworker: ocr\nexecuted_tool: text_extract\nresult:\n{}",
                output
            )));
        }

        if role.name() == "voice" && tool_set.contains("transcribe_audio") {
            let Some(path) = Self::extract_local_path(task) else {
                return Ok(Some(
                    "status: blocked\nworker: voice\nblockers: transcribe_audio needs a concrete local audio path".to_string(),
                ));
            };
            let path = path.to_string_lossy().to_string();
            let output =
                TranscribeTool::new("local-voice-runtime", None, Self::empty_sensory_hub())
                    .call(&json!({ "file_path": path }).to_string())
                    .await?;
            return Ok(Some(format!(
                "status: completed\nworker: voice\nexecuted_tool: transcribe_audio\nresult:\n{}",
                output
            )));
        }

        if role.name() == "terminal" && tool_set.contains("command_exec") {
            if let Some(command) = Self::extract_terminal_command(task) {
                let workspace = self.data_dir.clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                });
                let runtime = if cfg!(windows) { "powershell" } else { "bash" };
                let output = CommandExecTool::new(workspace)
                    .call(
                        &json!({
                            "command": command,
                            "runtime": runtime,
                            "working_dir": ".",
                            "timeout_secs": 30,
                            "allow_network": false
                        })
                        .to_string(),
                    )
                    .await?;
                return Ok(Some(Self::format_command_exec_result(&output)));
            }
        }

        #[cfg(feature = "browser")]
        {
            if role.name() == "browser" && tool_set.contains("browser") {
                if let Some(url) = Self::first_url(task) {
                    let output = BrowserTool::snapshot_once(&url, true, true).await?;
                    let snapshot_payload = serde_json::json!({
                        "url": url,
                        "content": output,
                        "content_quality": "actionable",
                        "orchestration_decision": {
                            "can_finalize_answer": true
                        },
                        "verification_followup": {
                            "answer_readiness": "source_content_observed"
                        }
                    })
                    .to_string();
                    let result_summary =
                        Self::compact_collection_fetch_summary(task, &url, &snapshot_payload)
                            .map(|summary| format!("result_summary:\n{summary}\n"))
                            .unwrap_or_default();
                    let status = if Self::task_requires_verified_fetch_result(task)
                        && !Self::collection_summary_ready_for_completion(task, &result_summary)
                    {
                        "blocked"
                    } else {
                        "completed"
                    };
                    let blocker = if status == "blocked" {
                        let reason = Self::collection_intent_alignment_blocker(
                            task,
                            &url,
                            &snapshot_payload,
                        )
                        .unwrap_or_else(|| {
                            "browser page did not expose enough item-level public metadata"
                                .to_string()
                        });
                        format!("blockers: {reason}\n")
                    } else {
                        String::new()
                    };
                    return Ok(Some(format!(
                        "status: {status}\nworker: browser\nexecuted_tool: browser_browse\nsource_url: {}\n{}{}result:\n{}",
                        url, blocker, result_summary, output
                    )));
                }

                if Self::task_requests_lookup(task) {
                    return Self::try_browser_lookup(task).await;
                }
            }
        }

        if tool_set.contains("crypto") || tool_set.contains("cipher") {
            let lowered = task.to_ascii_lowercase();
            if lowered.contains("sha256") || lowered.contains("hash") || task.contains("哈希") {
                let text = Self::extract_hash_text(task).unwrap_or_else(|| task.trim().to_string());
                let output = CipherTool
                    .call(
                        &json!({
                            "action": "hash_text",
                            "text": text,
                            "algorithm": if lowered.contains("sha512") { "sha512" } else { "sha256" }
                        })
                        .to_string(),
                    )
                    .await?;
                return Ok(Some(format!(
                    "status: completed\nworker: {}\nexecuted_tool: cipher\nresult:\n{}",
                    role.name(),
                    output
                )));
            }
        }

        if tool_set.contains("data") || tool_set.contains("data_transform") {
            let lowered = task.to_ascii_lowercase();
            if lowered.contains("stats")
                || lowered.contains("summary")
                || lowered.contains("mean")
                || lowered.contains("count")
                || task.contains("统计")
                || task.contains("均值")
                || task.contains("数量")
            {
                let numbers = Self::extract_inline_numbers(task);
                if !numbers.is_empty() {
                    let rows = numbers
                        .into_iter()
                        .map(|value| json!({ "value": value }))
                        .collect::<Vec<_>>();
                    let output = DataTransformTool::new(role.name())
                        .call(
                            &json!({
                                "action": "stats",
                                "data": rows,
                                "columns": ["value"]
                            })
                            .to_string(),
                        )
                        .await?;
                    return Ok(Some(format!(
                        "status: completed\nworker: {}\nexecuted_tool: data_transform\nresult:\n{}",
                        role.name(),
                        output
                    )));
                }
            }
        }

        if role.name() == "researcher"
            && tool_set.contains("web_search")
            && Self::task_requests_lookup(task)
        {
            let prefers_academic = Self::task_prefers_academic_sources(task);
            let prefers_structured_sources = Self::task_prefers_structured_sources(task);
            let requires_structured_followup = Self::task_requires_structured_followup(task);
            let mut structured_fallback_note = None;
            if tool_set.contains("web_fetch") {
                let structured_urls = Self::structured_lookup_urls(task);
                if !structured_urls.is_empty() {
                    let fetch = WebFetchTool::with_defaults()?;
                    let mut last_structured_blocker = None;
                    for url in structured_urls {
                        let fetched = match fetch
                            .call(
                                &json!({
                                    "url": url,
                                    "structured": true
                                })
                                .to_string(),
                            )
                            .await
                        {
                            Ok(fetched) => fetched,
                            Err(error) => {
                                last_structured_blocker = Some(format!(
                                    "structured source fetch failed for {url}: {error}"
                                ));
                                continue;
                            }
                        };
                        let structured_discovery_can_seed_followup =
                            Self::structured_discovery_result_can_seed_followup(&url, &fetched);
                        let structured_result_matches_task =
                            Self::fetched_result_looks_usable_for_task(task, &fetched);
                        if structured_result_matches_task || structured_discovery_can_seed_followup
                        {
                            if structured_result_matches_task {
                                if let Some(compact_result) =
                                    Self::compact_structured_fetch_result(task, &fetched, 2)
                                {
                                    return Ok(Some(compact_result));
                                }
                            }
                            let followup_urls =
                                Self::fetched_result_followup_urls(task, &fetched, 2);
                            if !followup_urls.is_empty() {
                                for followup_url in followup_urls {
                                    let followup_fetched = match fetch
                                        .call(
                                            &json!({
                                                "url": followup_url,
                                                "structured": true
                                            })
                                            .to_string(),
                                        )
                                        .await
                                    {
                                        Ok(fetched) => fetched,
                                        Err(error) => {
                                            last_structured_blocker = Some(format!(
                                                "structured follow-up fetch failed for {followup_url}: {error}"
                                            ));
                                            continue;
                                        }
                                    };
                                    let combined = Self::format_research_fetch_completion(
                                        task,
                                        &followup_url,
                                        None,
                                        Some("structured_source_first"),
                                        &fetched,
                                        &followup_fetched,
                                    );
                                    if Self::fetched_result_looks_usable_for_task(
                                        task,
                                        &followup_fetched,
                                    ) {
                                        return Ok(Some(combined));
                                    }
                                    if Self::task_prefers_academic_sources(task)
                                        && Self::url_is_specific_academic_record(&followup_url)
                                        && Self::fetched_result_looks_usable(&followup_fetched)
                                    {
                                        return Ok(Some(combined));
                                    }
                                }
                                continue;
                            }
                            if requires_structured_followup
                                && Self::is_structured_discovery_url(&url)
                            {
                                continue;
                            }
                            return Ok(Some(Self::format_research_fetch_completion(
                                task,
                                &url,
                                None,
                                Some("structured_source_first"),
                                "",
                                &fetched,
                            )));
                        } else {
                            last_structured_blocker = Self::fetched_result_blocker(&fetched);
                        }
                    }
                    if requires_structured_followup {
                        structured_fallback_note = Some(
                            "structured source discovery returned no usable follow-up records"
                                .to_string(),
                        );
                        tracing::debug!(
                            "researcher structured lookup produced no usable follow-up records; falling back to search/browser"
                        );
                    }
                    if let Some(blocker) = last_structured_blocker {
                        tracing::debug!(
                            "researcher structured lookup failed; falling back to web_search: {}",
                            blocker
                        );
                    }
                }
            }
            if requires_structured_followup && prefers_structured_sources {
                structured_fallback_note.get_or_insert_with(|| {
                    "structured source lookup was preferred, but no structured fetch path was available"
                        .to_string()
                });
            }
            let search = if Self::task_requests_collection_or_ranking(task) {
                WebSearchTool::new(WebSearchConfig {
                    max_results: Self::requested_collection_item_count(task).clamp(5, 10) as u8,
                    ..WebSearchConfig::default()
                })?
            } else {
                WebSearchTool::from_env()?
            };
            let queries = Self::lookup_query_variants(task);
            let mut last_output = None;
            let mut last_query = None;

            for query in queries {
                let output = match search
                    .call(
                        &json!({
                            "query": query,
                            "structured": true
                        })
                        .to_string(),
                    )
                    .await
                {
                    Ok(output) => output,
                    Err(error) => {
                        if let Some(blocker) = Self::summarize_lookup_blocker(&error) {
                            #[cfg(feature = "browser")]
                            if tool_set.contains("browser") {
                                if let Some(result) =
                                    Self::try_browser_lookup_for_worker(task, "researcher").await?
                                {
                                    if !Self::looks_like_worker_blocker_status(&result) {
                                        return Ok(Some(result));
                                    }
                                    last_output = Some(format!(
                                        "{result}\n\nstructured_fallback_note: {}",
                                        structured_fallback_note.as_deref().unwrap_or("")
                                    ));
                                    continue;
                                }
                            }
                            return Ok(Some(format!(
                                "status: blocked\nworker: researcher\nblockers: {}\nquery: {}",
                                blocker, query
                            )));
                        }
                        return Err(error);
                    }
                };

                last_query = Some(query);
                let usable = Self::search_output_has_usable_candidates(task, &output);
                let has_preferred_academic =
                    Self::search_output_has_preferred_academic_candidates(&output);
                last_output = Some(output.clone());

                if !usable
                    && Self::task_requests_data_or_records(task)
                    && tool_set.contains("web_fetch")
                {
                    let fetch = WebFetchTool::with_defaults()?;
                    let discovery_urls = Self::best_discovery_fetch_urls(task, &output, 2);
                    let mut last_fetch_error = None;
                    for discovery_url in discovery_urls {
                        let discovery_fetched = match fetch
                            .call(
                                &json!({
                                    "url": discovery_url,
                                    "structured": true
                                })
                                .to_string(),
                            )
                            .await
                        {
                            Ok(fetched) => fetched,
                            Err(error) => {
                                last_fetch_error = Some(format!(
                                    "discovery fetch failed for {discovery_url}: {error}"
                                ));
                                continue;
                            }
                        };
                        let followup_urls =
                            Self::fetched_result_followup_urls(task, &discovery_fetched, 3);
                        for followup_url in followup_urls {
                            let followup_fetched = match fetch
                                .call(
                                    &json!({
                                        "url": followup_url,
                                        "structured": true
                                    })
                                    .to_string(),
                                )
                                .await
                            {
                                Ok(fetched) => fetched,
                                Err(error) => {
                                    last_fetch_error = Some(format!(
                                        "follow-up fetch failed for {followup_url}: {error}"
                                    ));
                                    continue;
                                }
                            };
                            if Self::fetched_result_looks_usable_for_task(task, &followup_fetched) {
                                return Ok(Some(Self::format_research_fetch_completion(
                                    task,
                                    &followup_url,
                                    last_query.as_deref(),
                                    Some("data_discovery_followup"),
                                    &format!(
                                        "search_result:\n{}\n\ndiscovery_result:\n{}",
                                        output, discovery_fetched
                                    ),
                                    &followup_fetched,
                                )));
                            }
                        }
                    }
                    if let Some(error) = last_fetch_error {
                        last_output = Some(format!(
                            "status: blocked\nworker: researcher\nblockers: {}\nsearch_query: {}\nsearch_result:\n{}",
                            error,
                            last_query.as_deref().unwrap_or(""),
                            output
                        ));
                    }
                }

                if prefers_academic && usable && !has_preferred_academic {
                    continue;
                }
                if tool_set.contains("web_fetch")
                    && ((usable && Self::search_output_requires_followup(&output))
                        || (Self::task_requests_collection_or_ranking(task)
                            && Self::search_output_has_any_url_candidates(&output)))
                {
                    let fetch = WebFetchTool::with_defaults()?;
                    let candidates = Self::best_followup_fetch_urls(
                        task,
                        &output,
                        Self::followup_fetch_limit_for_task(task),
                    );
                    let mut last_fetched = None;
                    let mut last_fetch_error = None;

                    for url in candidates {
                        let fetched = match fetch
                            .call(
                                &json!({
                                    "url": url,
                                    "structured": true
                                })
                                .to_string(),
                            )
                            .await
                        {
                            Ok(fetched) => fetched,
                            Err(error) => {
                                last_fetch_error =
                                    Some(format!("candidate fetch failed for {url}: {error}"));
                                continue;
                            }
                        };
                        let combined = Self::format_research_fetch_completion(
                            task,
                            &url,
                            last_query.as_deref(),
                            None,
                            &output,
                            &fetched,
                        );
                        last_fetched = Some(combined.clone());

                        if Self::fetched_result_looks_usable_for_task(task, &fetched) {
                            if Self::task_requests_data_or_records(task)
                                && Self::url_looks_like_homepage(&url)
                            {
                                let followup_urls =
                                    Self::fetched_result_followup_urls(task, &fetched, 2);
                                let mut followup_completed = None;
                                for followup_url in followup_urls {
                                    let followup_fetched = match fetch
                                        .call(
                                            &json!({
                                                "url": followup_url,
                                                "structured": true
                                            })
                                            .to_string(),
                                        )
                                        .await
                                    {
                                        Ok(fetched) => fetched,
                                        Err(error) => {
                                            last_fetch_error = Some(format!(
                                                "follow-up fetch failed for {followup_url}: {error}"
                                            ));
                                            continue;
                                        }
                                    };
                                    if Self::fetched_result_looks_usable_for_task(
                                        task,
                                        &followup_fetched,
                                    ) {
                                        followup_completed =
                                            Some(Self::format_research_fetch_completion(
                                                task,
                                                &followup_url,
                                                last_query.as_deref(),
                                                None,
                                                &output,
                                                &followup_fetched,
                                            ));
                                        break;
                                    }
                                }
                                if let Some(completed) = followup_completed {
                                    return Ok(Some(completed));
                                }
                                last_fetched = Some(combined);
                                continue;
                            }
                            return Ok(Some(combined));
                        }
                    }

                    if let Some(fallback) = last_fetched {
                        if let Some(search_index_completion) =
                            Self::format_search_index_collection_completion(
                                task,
                                last_query.as_deref(),
                                &output,
                            )
                        {
                            if !Self::looks_like_worker_blocker_status(&search_index_completion) {
                                return Ok(Some(search_index_completion));
                            }
                            last_output = Some(search_index_completion);
                            continue;
                        }
                        if Self::task_requires_verified_fetch_result(task)
                            || Self::search_output_requires_followup(&output)
                            || Self::fetched_result_requires_more_evidence(&fallback)
                        {
                            last_output = Some(fallback);
                            continue;
                        }
                        return Ok(Some(fallback));
                    }
                    if let Some(error) = last_fetch_error {
                        last_output = Some(format!(
                            "status: blocked\nworker: researcher\nblockers: {}\nsearch_query: {}\nsearch_result:\n{}",
                            error,
                            last_query.as_deref().unwrap_or(""),
                            output
                        ));
                        continue;
                    }
                }

                if usable {
                    return Ok(Some(format!(
                        "status: completed\nworker: researcher\nexecuted_tool: web_search\nsearch_query: {}\nresult:\n{}",
                        last_query.as_deref().unwrap_or(""),
                        output
                    )));
                }
            }

            #[cfg(feature = "browser")]
            if tool_set.contains("browser") {
                if let Some(result) =
                    Self::try_browser_lookup_for_worker(task, "researcher").await?
                {
                    if !Self::looks_like_worker_blocker_status(&result) {
                        return Ok(Some(result));
                    }
                    last_output = Some(format!(
                        "{result}\n\nstructured_fallback_note: {}",
                        structured_fallback_note.as_deref().unwrap_or("")
                    ));
                }
            }

            if let Some(output) = last_output
                .as_deref()
                .filter(|output| Self::looks_like_worker_blocker_status(output))
            {
                return Ok(Some(output.to_string()));
            }

            if Self::task_requests_data_or_records(task) {
                return Ok(Some(format!(
                    "status: blocked\nworker: researcher\nblockers: no specific data or record page was found; only directory/search pages were available\nquery: {}",
                    last_query.as_deref().unwrap_or("")
                )));
            }

            if Self::task_requests_collection_or_ranking(task) {
                if let Some(output) = last_output.as_deref() {
                    if Self::looks_like_worker_blocker_status(output) {
                        return Ok(Some(output.to_string()));
                    }
                    if let Some(search_index_completion) =
                        Self::format_search_index_collection_completion(
                            task,
                            last_query.as_deref(),
                            output,
                        )
                    {
                        return Ok(Some(search_index_completion));
                    }
                }
                return Ok(Some(format!(
                    "status: blocked\nworker: researcher\nblockers: collection/ranking task requires at least {} item-level records, but only directory/search pages or insufficient page evidence were available\nquery: {}",
                    Self::requested_collection_item_count(task),
                    last_query.as_deref().unwrap_or("")
                )));
            }

            if let (Some(query), Some(output)) = (last_query, last_output) {
                if Self::looks_like_worker_blocker_status(&output) {
                    return Ok(Some(output));
                }
                return Ok(Some(format!(
                    "status: completed\nworker: researcher\nexecuted_tool: web_search\nsearch_query: {}\nresult:\n{}",
                    query, output
                )));
            }
        }

        if role.name() == "writer" && Self::task_requests_file_write(task) {
            if Self::writer_fast_path_should_defer_existing_revision(task) {
                return Ok(None);
            }
            if Self::should_route_writer_fiction_to_novel_studio(blueprint_tools, task) {
                return Ok(None);
            }
            if let Some(result) = self
                .write_longform_continuation_for_delegate(task, role.name())
                .await?
            {
                return Ok(Some(result));
            }
            if let Some(result) = self
                .write_local_file_for_delegate(task, role.name())
                .await?
            {
                return Ok(Some(result));
            }
        }

        if (role.name() == "writer" || role.name() == "coder")
            && Self::task_requests_file_read(task)
            && !Self::should_route_writer_fiction_to_novel_studio(blueprint_tools, task)
        {
            if let Some(result) = Self::read_local_file_for_delegate(task, role.name())? {
                return Ok(Some(result));
            }
        }

        if role.name() == "knowledge"
            && (tool_set.contains("knowledge") || tool_set.contains("knowledge_manage_document"))
            && Self::task_requests_knowledge_management(task)
        {
            let Some(search_engine) = &self.search_engine else {
                return Ok(None);
            };
            let manager = KnowledgeManageDocumentTool::new(search_engine.clone());
            let task_upper = task.to_ascii_uppercase();
            if task_upper.contains("UPDATE ") {
                if let Some((collection, path)) =
                    Self::extract_management_confirmation(task, "UPDATE")
                {
                    let Some(content) = Self::extract_update_content(task) else {
                        return Ok(Some(format!(
                            "status: blocked\nworker: knowledge\nexecuted_tool: knowledge_manage_document\nblockers: update confirmation was provided but replacement content was missing\nrequired_confirmation: UPDATE {}/{}\nrequired_content_marker: 新内容：<replacement text>",
                            collection, path
                        )));
                    };
                    let output = manager
                        .call(
                            &json!({
                                "action": "update",
                                "collection": collection,
                                "path": path,
                                "content": content,
                                "confirmation_phrase": format!("UPDATE {}/{}", collection, path)
                            })
                            .to_string(),
                        )
                        .await?;
                    return Ok(Some(format!(
                        "status: completed\nworker: knowledge\nexecuted_tool: knowledge_manage_document\nresult:\n{}",
                        output
                    )));
                }
            }

            if task_upper.contains("DELETE ") {
                if let Some((collection, path)) =
                    Self::extract_management_confirmation(task, "DELETE")
                {
                    let output = manager
                        .call(
                            &json!({
                                "action": "delete",
                                "collection": collection,
                                "path": path,
                                "confirmation_phrase": format!("DELETE {}/{}", collection, path)
                            })
                            .to_string(),
                        )
                        .await?;
                    return Ok(Some(format!(
                        "status: completed\nworker: knowledge\nexecuted_tool: knowledge_manage_document\nresult:\n{}",
                        output
                    )));
                }
            }

            let output = manager
                .call(
                    &json!({
                        "action": "search",
                        "query": task,
                        "limit": 5
                    })
                    .to_string(),
                )
                .await?;
            return Ok(Some(format!(
                "status: needs_confirmation\nworker: knowledge\nexecuted_tool: knowledge_manage_document\nresult:\n{}",
                output
            )));
        }

        if role.name() == "knowledge"
            && (tool_set.contains("knowledge") || tool_set.contains("knowledge_manage_document"))
            && Self::task_requests_knowledge_create(task)
        {
            let Some(search_engine) = &self.search_engine else {
                return Ok(None);
            };
            let manager = KnowledgeManageDocumentTool::new(search_engine.clone());
            let content = Self::extract_knowledge_create_content(task);
            let title = Self::infer_knowledge_create_title(&content);
            let output = manager
                .call(
                    &json!({
                        "action": "create",
                        "collection": "knowledge",
                        "title": title,
                        "content": content,
                    })
                    .to_string(),
                )
                .await?;
            return Ok(Some(format!(
                "status: completed\nworker: knowledge\nexecuted_tool: knowledge_manage_document\nresult:\n{}",
                output
            )));
        }

        if role.name() == "knowledge"
            && (tool_set.contains("knowledge") || tool_set.contains("tiered_search"))
            && Self::task_requests_knowledge_retrieval(task)
        {
            let mut result = "No results found.".to_string();
            if let Some(search_engine) = &self.search_engine {
                let queries = Self::knowledge_retrieval_queries(task);
                let exact_docs = Self::exact_knowledge_documents(search_engine, &queries)?;
                if !exact_docs.is_empty() {
                    result = Self::summarize_engram_documents(search_engine, &exact_docs);
                }
                for query in queries {
                    if result != "No results found." {
                        break;
                    }
                    let results = search_engine.search(&query, 5)?;
                    if !results.is_empty() {
                        result = Self::summarize_hybrid_knowledge_results(search_engine, &results);
                        break;
                    }
                }
            }
            if result == "No results found." {
                if let Some(memory) = &self.memory {
                    let docs = memory.search("default", None, task, 5).await?;
                    result = Self::summarize_knowledge_documents(&docs);
                } else {
                    return Ok(None);
                }
            }
            return Ok(Some(format!(
                "status: completed\nworker: knowledge\nexecuted_tool: tiered_search\nquery: {}\nresult:\n{}",
                task.trim(),
                result
            )));
        }

        if role.name() == "knowledge"
            && (tool_set.contains("knowledge") || tool_set.contains("knowledge_import_url"))
        {
            let Some(url) = Self::first_url(task) else {
                return Ok(Some(
                    "status: blocked\nworker: knowledge\nblockers: no concrete URL was provided for knowledge_import_url".to_string(),
                ));
            };
            if let Ok(parsed) = reqwest::Url::parse(&url) {
                if let Some(host) = parsed.host_str() {
                    let policy = policy_for_host(host);
                    if policy.challenge_prone && !policy.preferred_lookup_hosts.is_empty() {
                        return Ok(Some(format!(
                            "status: blocked\nworker: knowledge\nblockers: source host is challenge-prone and should be replaced with a more importable source first\nsource_url: {}\npreferred_alternatives: {}",
                            url,
                            policy.preferred_lookup_hosts.join(", ")
                        )));
                    }
                }
            }
            if let Some(blocker) = Self::knowledge_import_source_alignment_blocker(task) {
                return Ok(Some(format!(
                    "status: blocked\nworker: knowledge\nerror_kind: source_alignment_evidence_required\nsource_url: {}\nblockers: {}\nnext_step_hint: fetch/read the concrete source body or material evidence first, then import only the returned source_url plus fetched_result/body evidence if it matches the original request.",
                    url, blocker
                )));
            }
            let Some(search_engine) = &self.search_engine else {
                return Ok(None);
            };
            let importer = KnowledgeImportUrlTool::new(search_engine.clone());
            let output = importer
                .call(
                    &json!({
                        "url": url,
                        "collection": "references"
                    })
                    .to_string(),
                )
                .await?;
            return Ok(Some(format!(
                "status: completed\nworker: knowledge\nexecuted_tool: knowledge_import_url\nresult:\n{}",
                output
            )));
        }

        Ok(None)
    }

    async fn try_novel_content_operation_fast_path(
        &self,
        role: &AgentRole,
        task: &str,
    ) -> anyhow::Result<Option<String>> {
        let Some(project_path) = Self::extract_existing_artifact_project_path(task) else {
            return Ok(Some(
                "status: blocked\nworker: writer\nexecuted_tool: novel_studio\nblockers: missing project_path for novel content operation".to_string(),
            ));
        };
        let tool = NovelStudioTool::new(std::env::current_dir()?, role.name().to_string());
        if novel_content_fast_path_requests_project_status(task)
            && novel_content_fast_path_target_chapter_unspecified(task)
        {
            let raw = tool
                .call(
                    &json!({
                        "action": "status",
                        "project_path": project_path
                    })
                    .to_string(),
                )
                .await?;
            let value: serde_json::Value =
                serde_json::from_str(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
            if !value
                .get("success")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                return Ok(Some(format!(
                    "status: blocked\nworker: writer\nexecuted_tool: novel_studio\noperation: status\nproject_path: {project_path}\nblockers: novel_studio could not read project status\nresult: {}",
                    preview_text(&value.to_string(), 1600)
                )));
            }
            return Ok(Some(format_novel_content_fast_path_status(
                &project_path,
                &value,
            )));
        }

        let chapter_number = Self::requested_start_chapter(task).unwrap_or(1);
        if novel_content_fast_path_requests_deterministic_repair(task) {
            let raw = tool
                .call(
                    &json!({
                        "action": "revise_chapter",
                        "project_path": project_path,
                        "chapter_number": chapter_number,
                        "revision_notes": "Apply deterministic title/metadata/body surface cleanup for the requested chapter without rewriting the prose.",
                        "status": "revised"
                    })
                    .to_string(),
                )
                .await?;
            let value: serde_json::Value =
                serde_json::from_str(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
            if !value
                .get("success")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                return Ok(Some(format!(
                    "status: blocked\nworker: writer\nexecuted_tool: novel_studio\noperation: revise_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nblockers: novel_studio could not repair the requested chapter\nresult: {}",
                    preview_text(&value.to_string(), 1600)
                )));
            }
            return Ok(Some(format_novel_content_fast_path_revision(
                &project_path,
                chapter_number,
                &value,
            )));
        }

        if !task.contains("操作类型：查询章节内容") {
            return Ok(Some(format!(
                "status: blocked\nworker: writer\nexecuted_tool: novel_studio\noperation: content_mutation\nproject_path: {project_path}\nchapter_number: {chapter_number}\nblockers: content mutation requires a generated revised chapter body; deterministic fast path refused to continue a new chapter\nnext_action: route the request through the writer model to read_chapter, generate a revised body, then call revise_chapter"
            )));
        }

        let raw = tool
            .call(
                &json!({
                    "action": "read_chapter",
                    "project_path": project_path,
                    "chapter_number": chapter_number
                })
                .to_string(),
            )
            .await?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
        if !value
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return Ok(Some(format!(
                "status: blocked\nworker: writer\nexecuted_tool: novel_studio\noperation: read_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nblockers: novel_studio could not read the requested chapter\nresult: {}",
                preview_text(&value.to_string(), 1600)
            )));
        }

        let artifact_path = value
            .get("artifact_path")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let chapter = value.get("chapter").cloned().unwrap_or_else(|| json!({}));
        let title = chapter
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let summary = chapter
            .get("summary")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                value
                    .get("content")
                    .and_then(|value| value.as_str())
                    .map(|content| preview_text(content, 500))
                    .unwrap_or_else(|| "未找到章节摘要。".to_string())
            });
        let key_facts = chapter
            .get("key_facts")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .take(8)
                    .collect::<Vec<_>>()
                    .join("；")
            })
            .unwrap_or_default();
        let unit_count = chapter
            .get("unit_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);

        Ok(Some(format!(
            "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: read_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nchapter_title: {title}\nunit_count: {unit_count}\nartifact_path: {artifact_path}\nruntime_effect: artifact.verified\nsummary: {summary}\nkey_facts: {key_facts}"
        )))
    }
}

fn novel_content_fast_path_requests_project_status(task: &str) -> bool {
    let lowered = task.to_ascii_lowercase();
    [
        "项目状态",
        "角色连续性",
        "人物连续性",
        "人物身份",
        "主角身份",
        "漂移",
        "哪些章节",
        "所有章节",
        "全部章节",
        "project status",
        "continuity",
        "character drift",
        "identity drift",
        "all chapters",
    ]
    .iter()
    .any(|term| task.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn novel_content_fast_path_requests_deterministic_repair(task: &str) -> bool {
    if !task.contains("操作类型：修改章节内容") {
        return false;
    }
    let lowered = task.to_ascii_lowercase();
    [
        "标题",
        "元数据",
        "格式",
        "污染",
        "乱码",
        "残留",
        "不要重写正文",
        "不重写正文",
        "只修复",
        "metadata",
        "format",
        "markup",
        "residue",
        "title",
        "do not rewrite",
    ]
    .iter()
    .any(|term| task.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn novel_content_fast_path_target_chapter_unspecified(task: &str) -> bool {
    let Some((_, tail)) = task.split_once("目标章节：") else {
        return true;
    };
    let target = tail.lines().next().unwrap_or_default();
    target.contains("未明确") || target.contains("不明确") || target.contains("未指定")
}

fn format_novel_content_fast_path_status(project_path: &str, value: &serde_json::Value) -> String {
    let state = value.get("state").cloned().unwrap_or_else(|| json!({}));
    let title = state
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let chapters = state
        .get("chapters")
        .or_else(|| state.get("chapter_count"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let approved_chapters = state
        .get("approved_chapters")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let first_unapproved = state
        .get("first_unapproved_chapter")
        .and_then(|value| value.as_u64())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());
    let blockers = value
        .get("identity_integrity_blockers")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().or_else(|| item.get("summary")?.as_str()))
                .take(12)
                .map(|item| format!("- {}", preview_text(item, 220)))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "none".to_string());

    format!(
        "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: status\nproject_path: {project_path}\nproject_title: {title}\nchapters: {chapters}\napproved_chapters: {approved_chapters}\nfirst_unapproved_chapter: {first_unapproved}\nidentity_integrity_blockers:\n{blockers}"
    )
}

fn format_novel_content_fast_path_revision(
    project_path: &str,
    chapter_number: usize,
    value: &serde_json::Value,
) -> String {
    let chapter = value.get("chapter").cloned().unwrap_or_else(|| json!({}));
    let title = chapter
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let unit_count = chapter
        .get("unit_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let artifact_path = value
        .get("artifact_path")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let txt_path = value
        .get("txt_artifact_path")
        .or_else(|| value.get("preferred_artifact_path"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let outcome = value
        .get("outcome_status")
        .and_then(|value| value.as_str())
        .unwrap_or("revised");
    let metadata_gate = value
        .get("metadata_gate")
        .map(|value| preview_text(&value.to_string(), 500))
        .unwrap_or_default();
    let quality_gate = value
        .get("quality_gate")
        .map(|value| preview_text(&value.to_string(), 500))
        .unwrap_or_default();
    format!(
        "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: revise_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nchapter_title: {title}\nunit_count: {unit_count}\nartifact_path: {artifact_path}\ntxt_artifact_path: {txt_path}\noutcome_status: {outcome}\nruntime_effect: artifact.verified\nruntime_effect: artifact.txt\nsummary: 已按用户要求对目标章节执行确定性标题/元数据/正文表面清洗，没有生成下一章。\nmetadata_gate: {metadata_gate}\nquality_gate: {quality_gate}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn novel_content_operation_detects_deterministic_metadata_repair() {
        let task = "\
[BENSHU_NOVEL_CONTENT_OPERATION]
操作类型：修改章节内容。必须先读取目标章节，再按用户要求改写相关内容。
目标章节：第3章
用户原话：第三章标题和正文格式有问题，请只修复第三章标题和格式污染，不要重写正文。";

        assert!(novel_content_fast_path_requests_deterministic_repair(task));
    }

    #[test]
    fn novel_content_operation_does_not_treat_semantic_revision_as_metadata_repair() {
        let task = "\
[BENSHU_NOVEL_CONTENT_OPERATION]
操作类型：修改章节内容。必须先读取目标章节，再按用户要求改写相关内容。
目标章节：第3章
用户原话：把第三章改成主角和导师发生争执，并增加一个新的伏笔。";

        assert!(!novel_content_fast_path_requests_deterministic_repair(task));
    }
}
