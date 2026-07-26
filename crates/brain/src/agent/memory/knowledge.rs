use chrono::Utc;
use redb::ReadableTable;

pub use benshu_memory_core::{
    traverse_related_facts, traverse_related_facts_with_report, Fact, FactProtection,
    FactReviewPayload, FactStatus, Relation, RelationQueryBudget, RelationTraversalReport,
    RelationTraversalResult, RELATION_QUERY_DEFAULT_MAX_DEPTH,
    RELATION_QUERY_DEFAULT_MAX_RETURNED_EDGES, RELATION_QUERY_DEFAULT_MAX_VISITED_NODES,
    RELATION_QUERY_HARD_CAP_DEPTH,
};

#[cfg(feature = "persistence")]
const FACT_REVIEW_META_PREFIX: &str = "fact.review.";

#[cfg(feature = "persistence")]
use crate::agent::memory::episodic::STM_METADATA_TABLE;
use crate::agent::memory::episodic::{ShortTermMemory, STM_FACTS_TABLE, STM_FACT_RELATIONS_TABLE};

impl ShortTermMemory {
    #[cfg(feature = "persistence")]
    fn fact_review_meta_key(fact_id: &str) -> String {
        format!("{FACT_REVIEW_META_PREFIX}{fact_id}")
    }

    #[cfg(not(feature = "persistence"))]
    pub async fn get_fact_review_payload_inner(
        &self,
        _fact_id: &str,
    ) -> crate::error::Result<Option<FactReviewPayload>> {
        Ok(None)
    }

    #[cfg(feature = "persistence")]
    pub async fn get_fact_review_payload_inner(
        &self,
        fact_id: &str,
    ) -> crate::error::Result<Option<FactReviewPayload>> {
        let Some(db) = &self.db else {
            return Ok(None);
        };
        let db = db.clone();
        let fact_id = fact_id.to_string();
        tokio::task::spawn_blocking(move || {
            let read_txn = db
                .begin_read()
                .map_err(|e| crate::error::Error::Internal(format!("redb read error: {}", e)))?;
            let table = read_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                crate::error::Error::Internal(format!("redb metadata table error: {}", e))
            })?;
            let key = Self::fact_review_meta_key(&fact_id);
            let value = table.get(key.as_str()).map_err(|e| {
                crate::error::Error::Internal(format!("redb metadata get error: {}", e))
            })?;
            let Some(value) = value else {
                return Ok(None);
            };
            let payload =
                serde_json::from_str::<FactReviewPayload>(value.value()).map_err(|e| {
                    crate::error::Error::Internal(format!(
                        "failed to decode fact review payload {}: {}",
                        fact_id, e
                    ))
                })?;
            Ok(Some(payload))
        })
        .await
        .map_err(|e| {
            crate::error::Error::Internal(format!("Get fact review payload panicked: {}", e))
        })?
    }

    #[cfg(not(feature = "persistence"))]
    pub async fn store_fact_review_payload_inner(
        &self,
        _fact_id: &str,
        _payload: &FactReviewPayload,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    #[cfg(feature = "persistence")]
    pub async fn store_fact_review_payload_inner(
        &self,
        fact_id: &str,
        payload: &FactReviewPayload,
    ) -> crate::error::Result<()> {
        let Some(db) = &self.db else {
            return Ok(());
        };
        let db = db.clone();
        let key = Self::fact_review_meta_key(fact_id);
        let data = serde_json::to_string(payload).map_err(|e| {
            crate::error::Error::Internal(format!("failed to encode fact review payload: {}", e))
        })?;
        tokio::task::spawn_blocking(move || {
            let write_txn = db
                .begin_write()
                .map_err(|e| crate::error::Error::Internal(format!("redb write error: {}", e)))?;
            {
                let mut table = write_txn.open_table(STM_METADATA_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb metadata table error: {}", e))
                })?;
                table.insert(key.as_str(), data.as_str()).map_err(|e| {
                    crate::error::Error::Internal(format!("redb metadata insert error: {}", e))
                })?;
            }
            write_txn.commit().map_err(|e| {
                crate::error::Error::Internal(format!("redb metadata commit error: {}", e))
            })?;
            Ok::<(), crate::error::Error>(())
        })
        .await
        .map_err(|e| {
            crate::error::Error::Internal(format!("Store fact review payload panicked: {}", e))
        })??;
        Ok(())
    }

    #[cfg(not(feature = "persistence"))]
    pub async fn update_fact_status_inner(
        &self,
        _fact_id: &str,
        _status: FactStatus,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    #[cfg(feature = "persistence")]
    pub async fn update_fact_status_inner(
        &self,
        fact_id: &str,
        status: FactStatus,
    ) -> crate::error::Result<()> {
        if let Some(db) = &self.db {
            let db = db.clone();
            let fact_id = fact_id.to_string();
            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_FACTS_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    let mut updates = Vec::new();
                    for entry in table.iter().map_err(|e| {
                        crate::error::Error::Internal(format!("redb iter error: {}", e))
                    })? {
                        let (key, value) = entry.map_err(|e| {
                            crate::error::Error::Internal(format!("redb entry error: {}", e))
                        })?;
                        let mut fact: Fact =
                            serde_json::from_slice(value.value()).map_err(|e| {
                                crate::error::Error::Internal(format!(
                                    "Failed to parse fact {}: {}",
                                    key.value(),
                                    e
                                ))
                            })?;
                        if fact.id == fact_id {
                            fact.status = status.clone();
                            fact.verified = matches!(status, FactStatus::Verified);
                            fact.updated_at = Utc::now();
                            updates.push((key.value().to_string(), fact));
                        }
                    }

                    for (key, fact) in updates {
                        let data = serde_json::to_vec(&fact).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "Failed to serialize fact {}: {}",
                                fact.id, e
                            ))
                        })?;
                        table.insert(key.as_str(), data.as_slice()).map_err(|e| {
                            crate::error::Error::Internal(format!("redb fact update error: {}", e))
                        })?;
                    }
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit fact error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("Update fact status panicked: {}", e))
            })??;
        }
        Ok(())
    }

    #[cfg(not(feature = "persistence"))]
    pub async fn update_fact_importance_by_id_inner(
        &self,
        _fact_id: &str,
        _importance: f32,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    #[cfg(feature = "persistence")]
    pub async fn update_fact_importance_by_id_inner(
        &self,
        fact_id: &str,
        importance: f32,
    ) -> crate::error::Result<()> {
        if let Some(db) = &self.db {
            let db = db.clone();
            let fact_id = fact_id.to_string();
            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_FACTS_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    let mut updates = Vec::new();
                    for entry in table.iter().map_err(|e| {
                        crate::error::Error::Internal(format!("redb iter error: {}", e))
                    })? {
                        let (key, value) = entry.map_err(|e| {
                            crate::error::Error::Internal(format!("redb entry error: {}", e))
                        })?;
                        let mut fact: Fact =
                            serde_json::from_slice(value.value()).map_err(|e| {
                                crate::error::Error::Internal(format!(
                                    "Failed to parse fact {}: {}",
                                    key.value(),
                                    e
                                ))
                            })?;
                        if fact.id == fact_id {
                            fact.importance = importance.clamp(0.0, 1.0);
                            fact.updated_at = Utc::now();
                            updates.push((key.value().to_string(), fact));
                        }
                    }

                    for (key, fact) in updates {
                        let data = serde_json::to_vec(&fact).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "Failed to serialize fact {}: {}",
                                fact.id, e
                            ))
                        })?;
                        table.insert(key.as_str(), data.as_slice()).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "redb fact importance update error: {}",
                                e
                            ))
                        })?;
                    }
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit fact error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("Update fact importance panicked: {}", e))
            })??;
        }
        Ok(())
    }

    #[cfg(not(feature = "persistence"))]
    pub async fn update_fact_protection_by_id_inner(
        &self,
        _fact_id: &str,
        _protection: FactProtection,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    #[cfg(feature = "persistence")]
    pub async fn update_fact_protection_by_id_inner(
        &self,
        fact_id: &str,
        protection: FactProtection,
    ) -> crate::error::Result<()> {
        if let Some(db) = &self.db {
            let db = db.clone();
            let fact_id = fact_id.to_string();
            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_FACTS_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    let mut updates = Vec::new();
                    for entry in table.iter().map_err(|e| {
                        crate::error::Error::Internal(format!("redb iter error: {}", e))
                    })? {
                        let (key, value) = entry.map_err(|e| {
                            crate::error::Error::Internal(format!("redb entry error: {}", e))
                        })?;
                        let mut fact: Fact =
                            serde_json::from_slice(value.value()).map_err(|e| {
                                crate::error::Error::Internal(format!(
                                    "Failed to parse fact {}: {}",
                                    key.value(),
                                    e
                                ))
                            })?;
                        if fact.id == fact_id {
                            fact.protection = protection.clone();
                            fact.updated_at = Utc::now();
                            updates.push((key.value().to_string(), fact));
                        }
                    }

                    for (key, fact) in updates {
                        let data = serde_json::to_vec(&fact).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "Failed to serialize fact {}: {}",
                                fact.id, e
                            ))
                        })?;
                        table.insert(key.as_str(), data.as_slice()).map_err(|e| {
                            crate::error::Error::Internal(format!(
                                "redb fact protection update error: {}",
                                e
                            ))
                        })?;
                    }
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit fact error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("Update fact protection panicked: {}", e))
            })??;
        }
        Ok(())
    }

    #[cfg(feature = "persistence")]
    pub async fn store_fact_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> crate::error::Result<()> {
        if let Some(db) = &self.db {
            let base_key = self.key(user_id, agent_id);
            let fact_key = format!("{}:{}", base_key, fact.id);
            let mut fact = fact;

            // Selective Encryption (Roadmap Phase 10)
            let sensitive_categories = [
                "secret",
                "private",
                "credential",
                "password",
                "bank",
                "identity",
            ];
            if sensitive_categories.contains(&fact.category.to_lowercase().as_str()) {
                if let Some(sec) = self.security.read().as_ref() {
                    if let Ok(encrypted) = sec.encrypt_fact(&fact.content) {
                        fact.content = encrypted;
                    }
                }
            }

            let data = serde_json::to_vec(&fact).map_err(|e| {
                crate::error::Error::Internal(format!(
                    "Failed to serialize fact {}: {}",
                    fact.id, e
                ))
            })?;
            let db = db.clone();
            let fact_id = fact.id.clone();
            let relations = fact.relations.clone();
            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn.open_table(STM_FACTS_TABLE).map_err(|e| {
                        crate::error::Error::Internal(format!("redb table error: {}", e))
                    })?;
                    table
                        .insert(fact_key.as_str(), data.as_slice())
                        .map_err(|e| {
                            crate::error::Error::Internal(format!("redb insert fact error: {}", e))
                        })?;

                    let mut rel_table =
                        write_txn
                            .open_table(STM_FACT_RELATIONS_TABLE)
                            .map_err(|e| {
                                crate::error::Error::Internal(format!(
                                    "redb rel table error: {}",
                                    e
                                ))
                            })?;
                    for rel in &relations {
                        let rel_key = format!("{}:{}:{}", fact_id, rel.predicate, rel.target_id);
                        let bytes = rel.strength.to_le_bytes();
                        rel_table
                            .insert(rel_key.as_str(), bytes.as_slice())
                            .map_err(|e| {
                                crate::error::Error::Internal(format!(
                                    "redb rel insert error: {}",
                                    e
                                ))
                            })?;
                    }
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit fact error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| crate::error::Error::Internal(format!("Store fact panicked: {}", e)))??;
            return Ok(());
        }
        Ok(())
    }

    #[cfg(feature = "persistence")]
    pub async fn retrieve_facts_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Fact>> {
        if let Some(db) = &self.db {
            let base_key = self.key(user_id, agent_id);
            let prefix = format!("{}:", base_key);
            let db = db.clone();
            let facts = tokio::task::spawn_blocking(move || {
                let read_txn = db.begin_read().map_err(|e| {
                    crate::error::Error::Internal(format!("redb read error: {}", e))
                })?;
                let table = read_txn.open_table(STM_FACTS_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb table error: {}", e))
                })?;

                let mut facts = Vec::new();
                let iterator = table.range(prefix.as_str()..).map_err(|e| {
                    crate::error::Error::Internal(format!("redb range error: {}", e))
                })?;

                for entry in iterator {
                    let (key, value) = entry.map_err(|e| {
                        crate::error::Error::Internal(format!("redb entry error: {}", e))
                    })?;
                    let key_str = key.value().to_string();

                    if !key_str.starts_with(&prefix) {
                        break;
                    }

                    let fact: Fact = serde_json::from_slice(value.value()).map_err(|e| {
                        crate::error::Error::Internal(format!(
                            "Failed to parse fact {}: {}",
                            key_str, e
                        ))
                    })?;
                    facts.push(fact);
                }
                Ok::<Vec<Fact>, crate::error::Error>(facts)
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("Retrieve facts panicked: {}", e))
            })??;

            let mut decrypted = Vec::with_capacity(facts.len());
            for mut fact in facts {
                if fact.content.starts_with("enc:") {
                    if let Some(sec) = self.security.read().as_ref() {
                        if let Ok(decrypted_content) = sec.decrypt_fact(&fact.content) {
                            fact.content = decrypted_content;
                        }
                    }
                }
                decrypted.push(fact);
            }
            return Ok(decrypted);
        }
        Ok(Vec::new())
    }

    #[cfg(feature = "persistence")]
    pub async fn find_related_facts_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        if let Some(db) = &self.db {
            let base_key = self.key(user_id, agent_id);
            let db = db.clone();
            let fact_id = fact_id.to_string();
            let fact_id_for_query = fact_id.clone();
            let results = tokio::task::spawn_blocking(move || {
                let read_txn = db.begin_read().map_err(|e| {
                    crate::error::Error::Internal(format!("redb read error: {}", e))
                })?;
                let fact_table = read_txn.open_table(STM_FACTS_TABLE).map_err(|e| {
                    crate::error::Error::Internal(format!("redb table error: {}", e))
                })?;

                let mut facts_by_id = std::collections::HashMap::new();
                for entry in fact_table
                    .iter()
                    .map_err(|e| crate::error::Error::Internal(format!("redb iter error: {}", e)))?
                {
                    let (key, value) = entry.map_err(|e| {
                        crate::error::Error::Internal(format!("redb entry error: {}", e))
                    })?;
                    if !key.value().starts_with(&base_key) {
                        continue;
                    }
                    let fact: Fact = serde_json::from_slice(value.value()).map_err(|e| {
                        crate::error::Error::Internal(format!("Failed to parse fact: {}", e))
                    })?;
                    facts_by_id.insert(fact.id.clone(), fact);
                }

                Ok::<RelationTraversalResult, crate::error::Error>(
                    traverse_related_facts_with_report(&facts_by_id, &fact_id_for_query, depth),
                )
            })
            .await
            .map_err(|e| {
                crate::error::Error::Internal(format!("Find related facts panicked: {}", e))
            })??;
            for (key, value) in results.report.metadata_entries(&fact_id) {
                self.set_metadata_inner(&key, &value).await?;
            }
            return Ok(results.facts);
        }

        Ok(Vec::new())
    }
}
