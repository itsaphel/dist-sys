use async_trait::async_trait;
use futures::FutureExt;
use log::{error, info};
use maelstrom::protocol::Message;
use maelstrom::{done, Node, Result, Runtime};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use tokio_context::context::Context;

pub(crate) fn main() -> Result<()> {
    Runtime::init(try_main())
}

async fn try_main() -> Result<()> {
    let handler = Arc::new(Handler::new());
    Runtime::new().with_handler(handler).run().await
}

#[derive(Clone)]
struct Handler {
    // List of messages received by this node
    messages: Arc<Mutex<Vec<u64>>>,
    // List of neighbours of this node
    neighbours: Arc<Mutex<Inner<String>>>,
    // A queue for unsent outbound messages
    queue: Arc<TokioMutex<Queue>>,
}

#[derive(Default)]
struct Inner<T> {
    vec: Vec<T>,
}

struct Queue {
    thread_running: bool,
    items: VecDeque<QueueItem>,
}

#[derive(Debug)]
struct QueueItem {
    node: String,
    message: u64,
}

impl Display for QueueItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "QueueItem [ node: {}, message: {} ]", self.node, self.message)
    }
}

// The handler contains implementations for various functions used by the `process` function of the
// Node trait. This is because we need a non-async function to use std's Mutex implementation.
// We could otherwise use Tokio's Mutex, which works in an async context, but is slower.
impl Handler {
    fn new() -> Self {
        let neighbours: Inner<String> = Inner::default();
        let queue = Queue {
            thread_running: false,
            items: VecDeque::new(),
        };
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            neighbours: Arc::new(Mutex::new(neighbours)),
            queue: Arc::new(TokioMutex::new(queue)),
        }
    }

    // Get a snapshot of messages on this node
    fn get_messages(&self) -> Vec<u64> {
        let messages = self.messages.lock()
            .expect("Could not get lock on messages");

        messages.clone()
    }

    // Add a message to messages. Returns whether the message was added (that is, whether it was
    // previously unseen).
    fn add_message(&self, message: u64) -> bool {
        let mut messages = self.messages.lock()
            .expect("Could not get mutable lock on messages");

        if !messages.contains(&message) {
            messages.push(message);
            return true;
        }
        false
    }

    // Return the neighbours of this node.
    fn get_neighbours(&self) -> Vec<String> {
        let neighbours = self.neighbours.lock()
            .expect("Failed to get lock on neighbours");

        neighbours.vec.clone()
    }

    // Replace the neighbours of this node.
    fn replace_neighbours(&self, new_neighbours: Vec<String>) {
        let mut neighbours = self.neighbours.lock()
            .expect("Could not lock neighbours for replacement");
        neighbours.vec = new_neighbours.clone();
    }

    // TODO: Optional runtime arg is a bit ugly. Let's refactor this.
    async fn add_failed_message(&self, to_node: String, message: u64, runtime: Option<Runtime>) {
        let mut queue = self.queue.lock().await;

        info!(
            "Adding message {} to {} to queue. Full queue: {}",
            message,
            to_node,
            queue.items.iter().map(|item| format!("{}", item)).collect::<Vec<String>>().join("\n")
        );

        queue.items.push_back(QueueItem {
            node: to_node,
            message,
        });

        if !queue.thread_running {
            self.spawn_recovery_thread(runtime.unwrap());
        }
    }

    fn spawn_recovery_thread(&self, runtime: Runtime) {
        let handler = self.clone();
        let runtime0 = runtime.clone();

        runtime.spawn(async move {
            let mut queue = handler.queue.lock().await;
            queue.thread_running = true;
            drop(queue);

            loop {
                let mut queue = handler.queue.lock().await;

                if queue.items.is_empty() {
                    queue.thread_running = false;
                    break;
                }

                let item = queue.items.pop_front()
                    .expect("Should be able to pop from front of non-empty queue");

                // Release lock on queue
                drop(queue);

                let (ctx, _handler) = Context::with_timeout(Duration::from_millis(100));
                let result = runtime0.call(
                    ctx,
                    item.node.clone(),
                    Request::Broadcast { message: item.message },
                ).await;

                if result.is_err() {
                    handler.add_failed_message(item.node, item.message, None).await;
                }
            }
        });
    }
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
                    // Gossip message to neighbours
                    let neighbours: Vec<String> = self.get_neighbours()
                        .into_iter()
                        .filter(|t: &String| t.as_str() != msg.src)
                        .collect();
                    for node in neighbours {
                        let (ctx, _handler) = Context::with_timeout(Duration::from_millis(100));

                        // Send message with retries
                        let runtime0 = runtime.clone();
                        let runtime1 = runtime.clone();
                        let handler0 = self.clone();
                        runtime.spawn(async move {
                            runtime0
                                .call(ctx, node.clone(), Request::Broadcast { message })
                                .then(|result| async move {
                                    if let Err(err) = result {
                                        error!("Error sending message {} to {}: {}", message, node, err);
                                        handler0.add_failed_message(node, message, Some(runtime1)).await;
                                    }
                                })
                                .await
                        });
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
