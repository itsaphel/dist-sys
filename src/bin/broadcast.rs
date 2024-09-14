use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use maelstrom::{Runtime, Result, Node, done};
use maelstrom::protocol::Message;
use serde::{Deserialize, Serialize};

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
}

// The handler contains implementations for various functions used by the `process` function of the
// Node trait. This is because we need a non-async function to use std's Mutex implementation.
// We could otherwise use Tokio's Mutex, which works in an async context, but is slower.
impl Handler {
    fn new() -> Self {
        let neighbours: Inner<String> = Inner::default();
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            neighbours: Arc::new(Mutex::new(neighbours)),
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
}

#[derive(Default)]
struct Inner<T> {
    vec: Vec<T>,
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
                    let neighbours = self.get_neighbours();
                    for node in neighbours {
                        runtime.call_async(node, Request::Broadcast { message });
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
