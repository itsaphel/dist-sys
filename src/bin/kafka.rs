use async_trait::async_trait;
use maelstrom::protocol::Message;
use maelstrom::{done, Node, Result, Runtime};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

const POLL_ITEM_LIMIT: usize = 10;
type TopicKey = String;

pub(crate) fn main() -> Result<()> {
    Runtime::init(try_main())
}

async fn try_main() -> Result<()> {
    let handler = Arc::new(Handler::default());
    Runtime::new().with_handler(handler).run().await
}

#[derive(Clone, Default)]
struct Handler {
    // TODO: Consider dashmap instead of RwLock<HashMap>>
    logs: Arc<RwLock<HashMap<TopicKey, ReplicationLog>>>,
}

#[derive(Clone, Default)]
struct ReplicationLog {
    inner: Arc<RwLock<ReplicationLogInner>>,
}

#[derive(Default)]
struct ReplicationLogInner {
    messages: BTreeMap<u64, u64>,
    committed_offset: u64,
}

impl Handler {
    fn get_or_create_log(&self, topic: TopicKey) -> ReplicationLog {
        let map = self.logs.read().unwrap();

        if let Some(log) = map.get(&topic) {
            log.clone()
        } else {
            // Upgrade our lock to a write lock and add a new replication log to the map
            drop(map);
            let mut map = self.logs.write().unwrap();

            let log = ReplicationLog::default();
            map.insert(topic, log.clone());
            log
        }
    }
}

impl ReplicationLog {
    /// Append a message to this replication log
    /// Returns the offset of the message
    fn append(&self, message: u64) -> u64 {
        let mut inner = self.inner.write().unwrap();

        let offset = match inner.messages.last_key_value() {
            Some((&key, _)) => key,
            None => 0,
        } + 1;
        inner.messages.insert(offset, message);

        offset
    }

    /// List up to `limit` many messages in the replication log, starting at the provided offset
    /// Returns a list of (offset, message) pairs
    fn list(&self, from_offset: u64, limit: usize) -> Vec<(u64, u64)> {
        let inner = self.inner.read().unwrap();

        inner.messages
            .range(from_offset..)
            .take(limit)
            .map(|(&offset, &msg)| (offset, msg))
            .collect()
    }

    /// Get the last committed offset
    fn get_committed_offset(&self) -> u64 {
        let inner = self.inner.read().unwrap();

        inner.committed_offset
    }

    /// Set the last committed offset
    fn set_committed_offset(&self, offset: u64) {
        let mut inner = self.inner.write().unwrap();
        inner.committed_offset = offset;
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum Request {
    Send { key: TopicKey, msg: u64 },
    Poll { offsets: HashMap<TopicKey, u64> },
    CommitOffsets { offsets: HashMap<TopicKey, u64> },
    ListCommittedOffsets { keys: Vec<TopicKey> },
}


#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum Response {
    SendOk { offset: u64 },
    PollOk { msgs: HashMap<TopicKey, Vec<(u64, u64)>> },
    ListCommittedOffsetsOk { offsets: HashMap<TopicKey, u64> },
}

#[async_trait]
impl Node for Handler {
    async fn process(&self, runtime: Runtime, msg: Message) -> Result<()> {
        let req: Result<Request> = msg.body.as_obj();

        match req {
            Ok(Request::Send { key: topic, msg: message }) => {
                let log = self.get_or_create_log(topic);
                let offset = log.append(message);
                runtime.reply(msg, Response::SendOk { offset }).await
            }
            Ok(Request::Poll { offsets: topics_with_offsets }) => {
                let messages: HashMap<TopicKey, Vec<(u64, u64)>> = topics_with_offsets
                    .into_iter()
                    .map(|(topic, from_offset)| {
                        let log = self.get_or_create_log(topic.clone());
                        (topic, log.list(from_offset, POLL_ITEM_LIMIT))
                    })
                    .collect();
                runtime.reply(msg, Response::PollOk { msgs: messages }).await
            }
            Ok(Request::CommitOffsets { offsets: topics_with_offsets }) => {
                topics_with_offsets
                    .into_iter()
                    .for_each(|(topic, offset)| {
                        let log = self.get_or_create_log(topic);
                        log.set_committed_offset(offset);
                    });
                runtime.reply_ok(msg).await
            }
            Ok(Request::ListCommittedOffsets { keys: topics }) => {
                let offsets: HashMap<TopicKey, u64> = topics
                    .into_iter()
                    .map(|topic| {
                        let log = self.get_or_create_log(topic.clone());
                        (topic, log.get_committed_offset())
                    })
                    .collect();
                runtime.reply(msg, Response::ListCommittedOffsetsOk { offsets }).await
            }
            _ => done(runtime, msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Handler, ReplicationLog};
    use std::sync::Arc;

    #[test]
    fn test_replication_log() {
        let log = ReplicationLog::default();

        // Initially it should have no messages and the committed offset should be 0
        assert!(log.list(0, 10).is_empty());
        assert_eq!(log.get_committed_offset(), 0);

        // When we append a message
        let offset = log.append(42);

        // Then
        assert_eq!(offset, 1);
        assert_eq!(log.list(0, 10), vec![(1, 42)]);

        // When we update the committed offset
        log.set_committed_offset(1);

        // Then
        assert_eq!(log.get_committed_offset(), 1);
    }

    #[test]
    fn test_handler() {
        let handler = Handler::default();

        // We can create a new log
        let log = handler.get_or_create_log("topic".to_string());
        assert!(log.list(0, 10).is_empty());

        // And interact with the log
        let offset = log.append(42);
        assert_eq!(offset, 1);
        assert_eq!(log.list(0, 10), vec![(1, 42)]);

        // And get the same log again
        let log2 = handler.get_or_create_log("topic".to_string());
        assert!(Arc::ptr_eq(&log.inner, &log2.inner));
    }
}