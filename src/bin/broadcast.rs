use std::collections::HashMap;
use std::sync::{Arc};
use async_trait::async_trait;
use maelstrom::{Runtime, Result, Node, done};
use maelstrom::protocol::Message;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub(crate) fn main() -> Result<()> {
    Runtime::init(try_main())
}

async fn try_main() -> Result<()> {
    let handler = Arc::new(Handler::new());
    Runtime::new().with_handler(handler).run().await
}

#[derive(Clone)]
struct Handler {
    messages: Arc<Mutex<Vec<u64>>>,
}

impl Handler {
    fn new() -> Self {
        Self {
            // TODO consider a RWLock and/or using parking_lot's implementation
            messages: Arc::new(Mutex::new(Vec::new())), // Initialise with empty vector
        }
    }
}

#[async_trait]
impl Node for Handler {
    async fn process(&self, runtime: Runtime, msg: Message) -> Result<()> {
        let req: Result<Request> = msg.body.as_obj();

        match req {
            Ok(Request::Broadcast { message }) => {
                // Lock the vec and mutate it
                let mut messages = self.messages.lock().await;
                messages.push(message);

                let response = Response::BroadcastOk;
                runtime.reply(msg, response).await
            }
            Ok(Request::Read) => {
                // Lock messages and clone for response
                let messages = self.messages.lock().await;
                let messages = messages.clone();
                
                let response = Response::ReadOk { messages };
                runtime.reply(msg, response).await
            }
            Ok(Request::Topology { .. }) => {
                let response = Response::TopologyOk;
                runtime.reply(msg, response).await
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
    BroadcastOk,
    TopologyOk,
}
