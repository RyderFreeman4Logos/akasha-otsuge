use super::*;
use crate::config::NotifierKind;
use crate::domain::{Domain, ReviewState, Scope};
use crate::incident::Priority;
use crate::storage::{Change, CommitRequest, RecordKey, RecordKind, hash};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Notification {
    schema_version: u32,
    notification_id: String,
    review_id: String,
    source_revision: u64,
    scope: Scope,
    state: ReviewState,
    priority: Priority,
    summary: String,
    evidence_locator: String,
    occurrences: u64,
    generation: u64,
    acknowledged_generation: Option<u64>,
    deliveries: BTreeMap<String, Delivery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delivery_error: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    status: String,
    generation: u64,
    config_generation: u64,
    external_message_id: String,
}

fn state_key(id: String) -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id,
    }
}

fn key(id: &str) -> Result<RecordKey> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error("INVALID_NOTIFICATION"));
    }
    Ok(state_key(format!("notification/item/{id}")))
}

impl Service {
    fn notification(&self, id: &str) -> Result<(u64, Notification)> {
        let row = self
            .storage
            .current(&key(id)?)?
            .ok_or(Error("NOTIFICATION_NOT_FOUND"))?;
        let item: Notification = serde_json::from_str(&row.content)?;
        if item.schema_version != 1 || item.notification_id != id || item.generation == 0 {
            return Err(Error("NOTIFICATION_CORRUPT"));
        }
        Ok((row.commit_seq, item))
    }

    fn notification_keys(&self, after: Option<&str>) -> Result<Vec<RecordKey>> {
        let after = after.map(key).transpose()?;
        Ok(self.storage.head_keys(
            &key(&"0".repeat(64))?,
            &state_key(format!("notification/item/{}", "g".repeat(64))),
            after.as_ref().map(|key| key.id.as_str()),
            32,
        )?)
    }

    async fn save_notification(&self, item: &Notification, previous: Option<u64>) -> Result<()> {
        self.storage
            .commit(&CommitRequest {
                request_id: format!(
                    "notification/{}/{}/{}/{}/{}",
                    item.notification_id,
                    item.source_revision,
                    previous.unwrap_or(0),
                    self.instance,
                    ipc::now_ms()?
                ),
                config_generation: self.config.pin().generation(),
                policy_revision: None,
                changes: vec![Change {
                    key: key(&item.notification_id)?,
                    expected_revision: previous,
                    content: serde_json::to_string(item)?,
                }],
            })
            .await?;
        Ok(())
    }

    // Revision outbox entries are committed atomically with their source review.
    async fn project_notifications(&self, scan: &mut u64) -> Result<()> {
        let cursor_key = state_key("notification/cursor".into());
        let cursor = self.storage.current(&cursor_key)?;
        let start = cursor
            .as_ref()
            .map(|r| serde_json::from_str::<u64>(&r.content))
            .transpose()?
            .unwrap_or(0)
            .max(*scan);
        let end = self.storage.sequence()?.min(start.saturating_add(32));
        let mut external = start;
        for seq in start + 1..=end {
            let entry = self
                .storage
                .outbox(seq)?
                .ok_or(Error("NOTIFICATION_CORRUPT"))?;
            for reference in entry.revisions {
                if !reference.key.id.starts_with("notification/") {
                    external = seq;
                }
                if reference.key.kind != RecordKind::State {
                    continue;
                }
                let (review_id, kind) = if let Some(id) = reference.key.id.strip_prefix("review/") {
                    (id, "review")
                } else if let Some(id) = reference.key.id.strip_prefix("incident/") {
                    (id, "incident")
                } else {
                    continue;
                };
                let id = hash(reference.key.id.as_bytes());
                let existing = match self.notification(&id) {
                    Ok(value) => Some(value),
                    Err(Error("NOTIFICATION_NOT_FOUND")) => None,
                    Err(error) => return Err(error),
                };
                if existing
                    .as_ref()
                    .is_some_and(|(_, item)| item.source_revision >= reference.commit_seq)
                {
                    continue;
                }
                let source = self
                    .storage
                    .revision(&reference.key, reference.commit_seq)?
                    .ok_or(Error("NOTIFICATION_CORRUPT"))?;
                let (scope, state, priority, occurrences) = if kind == "review" {
                    let review =
                        Domain::new(&self.storage).review_notification(review_id, source)?;
                    (review.scope, review.state, Priority::Normal, 1)
                } else {
                    let record: crate::incident::Record = serde_json::from_str(&source.content)?;
                    if record.schema_version != 1
                        || record.id != review_id
                        || record.occurrences == 0
                    {
                        return Err(Error("NOTIFICATION_CORRUPT"));
                    }
                    if !record.scope.contains(&record.scope) {
                        return Err(Error("NOTIFICATION_CORRUPT"));
                    }
                    record.signal.validate()?;
                    (
                        record.scope,
                        record.state,
                        record.priority,
                        record.occurrences,
                    )
                };
                let (previous, mut item) = if let Some((previous, mut item)) = existing {
                    if item.scope != scope {
                        return Err(Error("NOTIFICATION_CORRUPT"));
                    }
                    if item.state != state || item.priority != priority {
                        item.generation = item
                            .generation
                            .checked_add(1)
                            .ok_or(Error("NOTIFICATION_CORRUPT"))?;
                    }
                    (Some(previous), item)
                } else {
                    (
                        None,
                        Notification {
                            schema_version: 1,
                            notification_id: id,
                            review_id: review_id.into(),
                            source_revision: reference.commit_seq,
                            scope,
                            state,
                            priority,
                            summary: String::new(),
                            evidence_locator: format!("akasha-otsuge {kind} show {review_id}"),
                            occurrences,
                            generation: 1,
                            acknowledged_generation: None,
                            deliveries: BTreeMap::new(),
                            delivery_error: None,
                        },
                    )
                };
                item.source_revision = reference.commit_seq;
                item.state = state;
                item.priority = priority;
                item.occurrences = occurrences;
                item.summary = format!(
                    "{kind} is {} ({occurrences} reports)",
                    serde_json::to_value(state)?
                        .as_str()
                        .ok_or(Error("NOTIFICATION_CORRUPT"))?
                );
                self.save_notification(&item, previous).await?;
            }
        }
        // Our own writes also enter the outbox; never checkpoint only those writes.
        if external > start {
            self.storage
                .commit(&CommitRequest {
                    request_id: format!("notification/cursor/{start}/{external}"),
                    config_generation: self.config.pin().generation(),
                    policy_revision: None,
                    changes: vec![Change {
                        key: cursor_key,
                        expected_revision: cursor.map(|r| r.commit_seq),
                        content: external.to_string(),
                    }],
                })
                .await?;
        }
        *scan = end;
        Ok(())
    }

    pub(super) async fn advance_notifications(
        &self,
        scan: &mut u64,
        after: &mut Option<String>,
    ) -> Result<()> {
        let _guard = self.admission.lock().await;
        self.project_notifications(scan).await?;
        let keys = self.notification_keys(after.as_deref())?;
        let more = keys.len() > 32;
        for key in keys.into_iter().take(32) {
            let id = key
                .id
                .rsplit('/')
                .next()
                .ok_or(Error("NOTIFICATION_CORRUPT"))?;
            *after = Some(id.into());
            let (revision, mut item) = self.notification(id)?;
            match Domain::new(&self.storage)
                .permission(&jobs::context()?, Some(&item.scope))
                .map_err(Error::from)
            {
                Ok(_) => (),
                Err(Error("READ_DENIED")) => continue,
                Err(error) => return Err(error),
            }
            let mut changed = false;
            let mut delivery_error = None;
            let snapshot = self.config.pin();
            for name in snapshot.resolved().config().notifiers.keys() {
                let Some((generation, route)) = self.config.delivery_route(name) else {
                    continue;
                };
                if route.kind != NotifierKind::Local
                    || item
                        .deliveries
                        .get(name)
                        .is_some_and(|d| d.generation == item.generation)
                {
                    continue;
                }
                if !item.deliveries.contains_key(name) && item.deliveries.len() >= 64 {
                    delivery_error = Some("CHANNEL_HISTORY_LIMIT".into());
                    continue;
                }
                item.deliveries.insert(
                    name.clone(),
                    Delivery {
                        status: "sent".into(),
                        generation: item.generation,
                        config_generation: generation,
                        external_message_id: format!("local/{name}/{id}"),
                    },
                );
                changed = true;
            }
            if item.delivery_error != delivery_error {
                item.delivery_error = delivery_error;
                changed = true;
            }
            if changed {
                self.save_notification(&item, Some(revision)).await?;
            }
        }
        if !more {
            *after = None;
        }
        Ok(())
    }

    pub(super) async fn list_notifications(
        &self,
        scope: &Scope,
        after: Option<&str>,
    ) -> Result<Value> {
        let _guard = self.admission.lock().await;
        Domain::new(&self.storage).permission(&jobs::context()?, Some(scope))?;
        let keys = self.notification_keys(after)?;
        let more = keys.len() > 32;
        let mut items = vec![];
        let mut next = None;
        for key in keys.into_iter().take(32) {
            let id = key
                .id
                .rsplit('/')
                .next()
                .ok_or(Error("NOTIFICATION_CORRUPT"))?;
            let (_, item) = self.notification(id)?;
            if scope.contains(&item.scope) {
                items.push(item);
            }
            next = Some(id.to_owned());
        }
        Ok(json!({"items":items,"next":if more { next } else { None }}))
    }

    pub(super) async fn show_notification(&self, id: &str) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let (_, item) = self.notification(id)?;
        Domain::new(&self.storage).permission(&jobs::context()?, Some(&item.scope))?;
        Ok(serde_json::to_value(item)?)
    }

    pub(super) async fn ack_notification(
        &self,
        id: &str,
        generation: u64,
        request: &str,
    ) -> Result<Value> {
        let _guard = self.admission.lock().await;
        if !ipc::identifier(request) {
            return Err(Error("INVALID_REQUEST"));
        }
        let (revision, mut item) = self.notification(id)?;
        Domain::new(&self.storage).permission(&jobs::context()?, Some(&item.scope))?;
        let input = serde_json::to_string(&(id, generation))?;
        let request_id = format!("notification/ack/{}", hash(request.as_bytes()));
        let request_key = state_key(request_id.clone());
        if let Some(previous) = self.storage.current(&request_key)? {
            if previous.content != input {
                return Err(Error("IDEMPOTENCY_CONFLICT"));
            }
            return Ok(serde_json::to_value(
                self.storage
                    .receipt(&request_id)?
                    .ok_or(Error("NOTIFICATION_CORRUPT"))?,
            )?);
        }
        if generation != item.generation {
            return Err(Error("REVISION_CONFLICT"));
        }
        item.acknowledged_generation = Some(generation);
        let receipt = self
            .storage
            .commit(&CommitRequest {
                request_id,
                config_generation: self.config.pin().generation(),
                policy_revision: None,
                changes: vec![
                    Change {
                        key: key(id)?,
                        expected_revision: Some(revision),
                        content: serde_json::to_string(&item)?,
                    },
                    Change {
                        key: request_key,
                        expected_revision: None,
                        content: input,
                    },
                ],
            })
            .await?;
        Ok(serde_json::to_value(receipt)?)
    }
}
