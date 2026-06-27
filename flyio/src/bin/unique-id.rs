use std::sync::Arc;
use async_trait::async_trait;
use maelstrom::{Runtime, Result, Node, done};
use maelstrom::protocol::Message;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) fn main() -> Result<()> {
    Runtime::init(try_main())
}

async fn try_main() -> Result<()> {
    let handler = Arc::new(Handler::default());
    Runtime::new().with_handler(handler).run().await
}

#[derive(Clone, Default)]
struct Handler {}

#[async_trait]
impl Node for Handler {
    async fn process(&self, runtime: Runtime, req: Message) -> Result<()> {
        if req.get_type() == "generate" {
            let id = Uuid::now_v7();

            let res = Response::GenerateOk { id: id.to_string() };
            return runtime.reply(req, res).await
        }

        done(runtime, req)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum Response {
    GenerateOk { id: String },
}
