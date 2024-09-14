use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
    counter: Arc<AtomicU64>,
}

impl Handler {
    fn new() -> Self {
        Handler {
            counter: Arc::new(AtomicU64::new(0))
        }
    }
}

#[async_trait]
impl Node for Handler {
    async fn process(&self, runtime: Runtime, req: Message) -> Result<()> {
        if req.get_type() == "generate" {
            let id = self.counter.fetch_add(1, Ordering::SeqCst);

            let res = Response::GenerateOk { id };
            return runtime.reply(req, res).await
        }

        done(runtime, req)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum Response {
    GenerateOk { id: u64 },
}
