use super::core::Agent;
use crate::agent::evolution::distillation::MemoryDistiller;
use crate::agent::provider::Provider;
use benshu_infra::traits::resource::ResourceSensor;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

impl<P: Provider + 'static> Agent<P> {
    /// Start all background tasks for this agent
    pub fn start_background_tasks(&self) {
        if self.background_tasks_started.swap(true, Ordering::AcqRel) {
            return;
        }

        let runner = Arc::clone(&self.task_runner);
        let cancel_token = self.lifecycle_token();
        let agent_name = self.config.name.clone();
        runner.spawn(Box::pin(async move {
            cancel_token.cancelled().await;
            debug!(
                "{}: Background runtime supervisor shutting down",
                agent_name
            );
        }));

        if let Some(comm) = &self.comm_client {
            let runtime_profile = comm.runtime_profile();
            let agent = self.clone();
            tokio::spawn({
                let agent = agent.clone();
                async move {
                    agent
                        .set_comm_metadata_value("runtime_profile", runtime_profile.as_str())
                        .await;
                    agent
                        .set_comm_metadata_value(
                            "security_mode",
                            if agent
                                .comm_client
                                .as_ref()
                                .map(|client| client.security_enabled())
                                .unwrap_or(false)
                            {
                                "signed"
                            } else {
                                "unsigned"
                            },
                        )
                        .await;
                }
            });

            if runtime_profile.signing_required() && !comm.security_enabled() {
                warn!(
                    "{}: Comm runtime profile '{}' expects signed metadata, but no shared secret is configured",
                    self.config.name,
                    runtime_profile.as_str()
                );
            }

            if runtime_profile.receive_loop_enabled() {
                let cancel_token = self.lifecycle_token();
                let agent_name = self.config.name.clone();
                let agent = agent.clone();
                let runner = Arc::clone(&self.task_runner);

                runner.spawn(Box::pin(async move {
                    loop {
                        tokio::select! {
                            _ = cancel_token.cancelled() => {
                                info!("{}: Comm client background task shutting down", agent_name);
                                break;
                            }
                            result = agent.poll_comm_once() => {
                                if let Err(err) = result {
                                    warn!("{}: Comm client receive loop error: {}", agent_name, err);
                                }
                            }
                        }
                    }
                }));
                info!(
                    "{}: Comm client event polling task started",
                    self.config.name
                );
            }

            if runtime_profile.heartbeat_enabled() {
                let comm_hb = comm.clone();
                let cancel_token = self.lifecycle_token();
                let agent_id = self.config.name.clone();
                let runner = Arc::clone(&self.task_runner);
                runner.spawn(Box::pin(async move {
                    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                    loop {
                        tokio::select! {
                            _ = cancel_token.cancelled() => {
                                info!("{}: Swarm heartbeat task shutting down", agent_id);
                                break;
                            }
                            _ = interval.tick() => {
                                let hb_msg = benshu_comm::protocol::a2a::A2AMessage::Heartbeat {
                                    agent_id: agent_id.clone(),
                                    status: benshu_comm::protocol::a2a::AgentStatus::Online,
                                    load: 0.0,
                                    timestamp: chrono::Utc::now().timestamp() as u64,
                                };
                                let payload = serde_json::to_vec(&hb_msg).unwrap_or_default();
                                let _ = comm_hb.send_msg(benshu_comm::protocol::Address::System("all".to_string()), payload).await;
                            }
                        }
                    }
                }));
                info!(
                    "{}: Swarm heartbeat background task started",
                    self.config.name
                );
            }
        }

        if let Some(consolidator) = &self.sleep_consolidator {
            let consolidator = consolidator.clone();
            let cancel_token = self.lifecycle_token();
            let agent_name = self.config.name.clone();
            let runner = Arc::clone(&self.task_runner);
            runner.spawn(Box::pin(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30 * 60));
                loop {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            info!("{}: Sleep consolidator task shutting down", agent_name);
                            break;
                        }
                        _ = interval.tick() => {
                            info!("{}: Triggering sleep-consolidation cycle...", agent_name);
                            match consolidator.consolidate().await {
                                Ok(report) => info!("{}: Consolidation complete: {} reviewed, {} verified, {} pruned",
                                    agent_name, report.entries_reviewed, report.entries_verified, report.entries_pruned),
                                Err(e) => error!("{}: Consolidation failed: {}", agent_name, e),
                            }
                        }
                    }
                }
            }));
            info!(
                "{}: Sleep consolidator background task started",
                self.config.name
            );
        }

        if let Some(em) = &self.evolution_manager {
            em.start_worker();
            let em = em.clone();
            let cancel_token = self.lifecycle_token();
            let agent_name = self.config.name.clone();
            let runner = Arc::clone(&self.task_runner);
            runner.spawn(Box::pin(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5 * 60));
                loop {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            info!("{}: Evolution health watcher shutting down", agent_name);
                            break;
                        }
                        _ = interval.tick() => {
                            if let Err(e) = em.check_evolution_health().await {
                                error!("{}: Evolution health check failed: {}", agent_name, e);
                            }
                        }
                    }
                }
            }));
            info!("{}: Evolution health watcher started", self.config.name);
        }

        if let (Some(memory), Some(sensor_lock)) = (&self.memory, &self.sensor) {
            let mem = Arc::clone(memory);
            let sensor = Arc::clone(sensor_lock);
            let cancel_token = self.lifecycle_token();
            let agent_name = self.config.name.clone();
            let threshold = self.config.metabolic_threshold;
            let runner = Arc::clone(&self.task_runner);
            runner.spawn(Box::pin(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            info!("{}: Memory hygiene task shutting down", agent_name);
                            break;
                        }
                        _ = interval.tick() => {
                            let throttle = sensor.write().suggest_throttle_level(Some(threshold));
                            if throttle == crate::skills::ThrottleLevel::Low {
                                warn!("{}: Resource pressure HIGH (Throttle::Low). Skipping memory hygiene sweep.", agent_name);
                                continue;
                            }

                            if let Err(e) = mem.maintenance().await {
                                warn!("{}: Memory hygiene failed: {}", agent_name, e);
                            }
                        }
                    }
                }
            }));
            info!(
                "{}: Memory hygiene background task started",
                self.config.name
            );
        }

        if let (Some(cache), Some(sensor_lock)) = (&self.cache, &self.sensor) {
            let cache = Arc::clone(cache);
            let sensor = Arc::clone(sensor_lock);
            let cancel_token = self.lifecycle_token();
            let agent_name = self.config.name.clone();
            let threshold = self.config.metabolic_threshold;
            let runner = Arc::clone(&self.task_runner);
            runner.spawn(Box::pin(async move {
                let cleanup_interval = crate::agent::cache::cache_constants::CLEANUP_INTERVAL_SECS;
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(cleanup_interval));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            debug!("{}: Cache hygiene task shutting down", agent_name);
                            break;
                        }
                        _ = interval.tick() => {
                            let throttle = sensor.write().suggest_throttle_level(Some(threshold));
                            if throttle == crate::skills::ThrottleLevel::Low {
                                debug!("{}: Resource pressure HIGH. Skipping cache hygiene.", agent_name);
                                continue;
                            }
                            if let Err(e) = cache.background_cleanup().await {
                                warn!("{}: Cache hygiene failed: {}", agent_name, e);
                            }
                        }
                    }
                }
            }));
            info!(
                "{}: Cache hygiene background task started",
                self.config.name
            );
        }

        if let (Some(memory), Some(em), Some(sensor_lock)) =
            (&self.memory, &self.evolution_manager, &self.sensor)
        {
            let distiller = MemoryDistiller::new(
                Arc::clone(memory),
                em.auditor().provider(),
                em.auditor().model().to_string(),
            );
            let sensor = Arc::clone(sensor_lock);
            let cancel_token = self.lifecycle_token();
            let agent_name = self.config.name.clone();
            let threshold = self.config.metabolic_threshold;
            let runner = Arc::clone(&self.task_runner);
            runner.spawn(Box::pin(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600 * 4));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            info!("{}: Memory distillation task shutting down", agent_name);
                            break;
                        }
                        _ = interval.tick() => {
                            let throttle = sensor.write().suggest_throttle_level(Some(threshold));
                            if throttle == crate::skills::ThrottleLevel::Low {
                                warn!("{}: Resource pressure HIGH. Skipping heavy memory distillation.", agent_name);
                                continue;
                            }
                            match distiller.run().await {
                                Ok(count) if count > 0 => info!("{}: Memory distillation complete: {} sessions processed", agent_name, count),
                                Err(e) => warn!("{}: Memory distillation failed: {}", agent_name, e),
                                _ => {}
                            }
                        }
                    }
                }
            }));
            info!(
                "{}: Memory distillation background task started",
                self.config.name
            );
        }
    }

    pub fn active_background_tasks(&self) -> usize {
        let runtime_supervisor_active = self.background_tasks_started.load(Ordering::Acquire);
        self.task_runner
            .active_tasks()
            .max(usize::from(runtime_supervisor_active))
    }

    pub async fn shutdown(&self) {
        self.lifecycle_token().cancel();
        if let Some(em) = &self.evolution_manager {
            em.shutdown_worker().await;
        }
        self.task_runner.shutdown().await;
        self.background_tasks_started
            .store(false, Ordering::Release);
    }
}

impl<P: Provider> Drop for Agent<P> {
    fn drop(&mut self) {
        self.lifecycle_token.read().cancel();
        if let Some(em) = &self.evolution_manager {
            em.signal_shutdown();
        }
        debug!(
            "{}: Aborting all background tasks on agent drop",
            self.config.name
        );
        self.task_runner.abort_all();
    }
}
