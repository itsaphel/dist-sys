use async_trait::async_trait;
use log::{error, info};
use maelstrom::protocol::Message;
use maelstrom::{done, Node, Result, Runtime};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_context::context::Context;

const TIMEOUT_IN_MILLIS: u64 = 500;

pub(crate) fn main() -> Result<()> {
    Runtime::init(try_main())
}

async fn try_main() -> Result<()> {
    let handler = Arc::new(Handler::default());
    Runtime::new().with_handler(handler).run().await
}

#[derive(Clone, Default)]
struct Handler {
    // List of messages received by this node
    messages: Arc<Mutex<Vec<u64>>>,
    // List of neighbours of this node
    neighbours: Arc<Mutex<NeighboursInner>>,
    // A queue for unsent outbound messages
    queue: Arc<Mutex<Queue>>,
}

#[derive(Default)]
struct NeighboursInner {
    vec: Vec<String>,
}

#[derive(Default)]
struct Queue {
    thread_running: bool,
    items: VecDeque<QueueItem>,
}

struct QueueItem {
    node: String,
    message: u64,
}

impl Display for QueueItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[ node: {}, message: {} ]", self.node, self.message)
    }
}

/// The handler contains implementations for various functions used by the `process` function of the
/// Node trait. This is because we need a non-async function to use std's Mutex implementation.
/// We could otherwise use Tokio's Mutex, which works in an async context, but is slower.
impl Handler {
    /// Get a snapshot of messages on this node
    fn get_messages(&self) -> Vec<u64> {
        let messages = self.messages.lock()
            .expect("Could not get lock on messages");

        messages.clone()
    }

    /// Add a message to messages. Returns whether the message was added (that is, whether it was
    /// previously unseen).
    fn add_message(&self, message: u64) -> bool {
        let mut messages = self.messages.lock()
            .expect("Could not get mutable lock on messages");

        if !messages.contains(&message) {
            messages.push(message);
            return true;
        }
        false
    }

    /// Return the neighbours of this node.
    fn get_neighbours(&self) -> Vec<String> {
        let neighbours = self.neighbours.lock()
            .expect("Failed to get lock on neighbours");

        neighbours.vec.clone()
    }

    /// Replace the neighbours of this node.
    fn replace_neighbours(&self, new_neighbours: Vec<String>) {
        let mut neighbours = self.neighbours.lock()
            .expect("Could not lock neighbours for replacement");
        neighbours.vec = new_neighbours.clone();
    }

    /// Add a message to an outbox queue. The queue will keep trying to resend the message in a
    /// background thread until a successful response is received.
    fn add_message_to_queue(&self, to_node: String, message: u64, runtime: Runtime) {
        let mut queue = self.queue.lock()
            .expect("Could not get lock on queue");

        queue.items.push_back(QueueItem {
            node: to_node.clone(),
            message,
        });

        info!(
            "Adding message {} to {} to queue. Full queue: {}",
            message,
            to_node,
            queue.items.iter().map(|item| format!("{}", item)).collect::<Vec<String>>().join(", ")
        );

        if !queue.thread_running {
            spawn_recovery_thread(runtime, self.clone());
        }
    }
}

/// Send a message to a node with retries.
/// If the communication fails due to an error, it will be retried.
async fn send_message_with_retry(
    runtime: Runtime,
    handler: Handler,
    message: u64,
    node: String,
) {
    let (ctx, ctx_handle) = Context::with_timeout(Duration::from_millis(TIMEOUT_IN_MILLIS));
    let runtime0 = runtime.clone();

    // Keep ctx_handle in scope to avoid premature cancellation of the Context
    let _ctx_handle = ctx_handle;

    let result = runtime
        .call(ctx, node.clone(), Request::Broadcast { message })
        .await;

    if let Err(err) = result
    {
        error!("Error sending message {} to {}: {}", message, node, err);
        handler.add_message_to_queue(node, message, runtime0);
    }
}

fn spawn_recovery_thread(runtime: Runtime, handler: Handler) {
    // Both Handler and Runtime are implemented as Arc, so cloning just bumps the reference count
    // This is needed because we'll pass these variables to a new thread.
    let runtime0 = runtime.clone();

    runtime.spawn(async move {
        let mut queue = handler.queue.lock()
            .expect("Could not get lock on queue");
        queue.thread_running = true;
        drop(queue);

        loop {
            let mut queue = handler.queue.lock()
                .expect("Could not get lock on queue");

            if let Some(item) = queue.items.pop_front() {
                // Release lock on queue
                drop(queue);

                runtime0.spawn(
                    send_message_with_retry(runtime0.clone(), handler.clone(), item.message, item.node)
                );
            } else {
                // Terminate this thread if no items left in queue
                if queue.items.is_empty() {
                    queue.thread_running = false;
                    break;
                }
            }
        }
    });
}

#[async_trait]
impl Node for Handler {
    async fn process(&self, runtime: Runtime, msg: Message) -> Result<()> {
        let req: Result<Request> = msg.body.as_obj();

        match req {
            Ok(Request::Broadcast { message }) => {
                // Check if we've already seen the message
                // If we have, we've presumably already seen and gossip'd it, so should not send again
                if self.add_message(message) {
                    // Gossip message to neighbours, but exclude the sender of the broadcast
                    let neighbours: Vec<String> = self.get_neighbours()
                        .into_iter()
                        .filter(|t| t.as_str() != msg.src)
                        .collect();
                    for node in neighbours {
                        runtime.spawn(
                            send_message_with_retry(runtime.clone(), self.clone(), message, node)
                        );
                    }
                }

                runtime.reply_ok(msg).await
            }
            Ok(Request::Read) => {
                let messages = self.get_messages();
                let response = Response::ReadOk { messages };
                runtime.reply(msg, response).await
            }
            Ok(Request::Topology { topology }) => {
                let new_neighbours = topology.get(runtime.node_id()).unwrap();
                self.replace_neighbours(new_neighbours.clone());
                runtime.reply_ok(msg).await
            }
            _ => done(runtime, msg)
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum Request {
    Broadcast { message: u64 },
    Read,
    Topology { topology: HashMap<String, Vec<String>> },
}


#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum Response {
    ReadOk { messages: Vec<u64> },
}
