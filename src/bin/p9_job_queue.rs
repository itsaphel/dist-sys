use serde::{Deserialize, Serialize};
use serde_json::Value;
use smol::{
    Async, Timer,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
};
use std::{
    collections::{BinaryHeap, HashMap},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RequestType {
    Put,
    Get,
    Delete,
    Abort,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Error,
}

#[derive(Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
enum Request {
    #[serde(rename = "put")]
    Put { queue: String, job: Value, pri: u32 },

    #[serde(rename = "get")]
    Get {
        // TODO: Maybe just String
        queues: Vec<String>,
        #[serde(default)]
        wait: bool,
    },

    #[serde(rename = "delete")]
    Delete { id: i32 },

    #[serde(rename = "abort")]
    Abort { id: i32 },
}

#[derive(Serialize)]
struct ErrorResponse {
    #[serde(rename = "status")]
    status: Status,
    error: String,
}
impl ErrorResponse {
    fn new(msg: String) -> Self {
        Self {
            status: Status::Error,
            error: msg,
        }
    }
}

#[derive(Serialize)]
struct PutResponse {
    #[serde(rename = "status")]
    status: Status,
    id: i32,
}

#[derive(Serialize)]
#[serde(tag = "status")]
enum GetResponse {
    #[serde(rename = "ok")]
    Ok {
        id: i32,
        job: Value,
        pri: u32,
        queue: String,
    },
    #[serde(rename = "no-job")]
    NoJob,
}

#[derive(Serialize)]
#[serde(tag = "status")]
enum AbortOrDeleteResponse {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "no-job")]
    NoJob,
}

async fn handle(stream: Async<TcpStream>, addr: SocketAddr, ctx: Context) -> anyhow::Result<()> {
    //println!("Received new connection.");

    let (reader, mut writer) = smol::io::split(stream);
    let mut reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            //println!("[{}] Connection closed by client", addr);
            ctx.abort_any(&addr);
            return Ok(());
        }
        //println!("[{}] Request: {}", addr, line.trim());

        let req: Request = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                //println!("[{}] Bad request from client. {}", addr, e);
                let resp = serde_json::to_string(&ErrorResponse::new(e.to_string()))? + "\n";
                writer.write_all(resp.as_bytes()).await?;
                continue;
            }
        };

        let resp = match req {
            Request::Put { queue, job, pri } => {
                let id = ctx.add_pending_job(job, pri, queue);
                serde_json::to_string(&PutResponse {
                    status: Status::Ok,
                    id,
                })?
            }
            Request::Get { queues, wait } => {
                // TODO: Support wait

                let resp = if wait {
                    let job_resp;
                    loop {
                        match ctx.poll_job(&queues, addr) {
                            Some(job) => {
                                job_resp = GetResponse::Ok {
                                    id: job.id,
                                    job: job.data.clone(),
                                    pri: job.pri,
                                    queue: job.queue.clone(),
                                };
                                break;
                            }
                            None => {
                                Timer::after(Duration::from_millis(100)).await;
                            }
                        }
                    }
                    job_resp
                } else {
                    match ctx.poll_job(&queues, addr) {
                        Some(job) => GetResponse::Ok {
                            id: job.id,
                            job: job.data.clone(),
                            pri: job.pri,
                            queue: job.queue.clone(),
                        },
                        None => GetResponse::NoJob,
                    }
                };
                serde_json::to_string(&resp)?
            }
            Request::Delete { id } => {
                let resp = match ctx.try_delete(id) {
                    Ok(_) => AbortOrDeleteResponse::Ok,
                    Err(_) => AbortOrDeleteResponse::NoJob,
                };
                serde_json::to_string(&resp)?
            }
            Request::Abort { id } => {
                let resp = match ctx.try_abort(id, &addr) {
                    Ok(_) => AbortOrDeleteResponse::Ok,
                    Err(_) => AbortOrDeleteResponse::NoJob,
                };
                serde_json::to_string(&resp)?
            }
        } + "\n";
        //println!("[{}] Response: {}", addr, resp.trim());
        writer.write_all(resp.as_bytes()).await?;
    }
}

struct Job {
    id: i32,
    pri: u32,
    data: Value,
    queue: String,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Job {}
impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.pri.partial_cmp(&other.pri)
    }
}
impl Ord for Job {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.pri.cmp(&other.pri)
    }
}

#[derive(Clone, Default)]
struct Context {
    inner: Arc<Mutex<ContextInner>>,
}

#[derive(Default)]
struct ContextInner {
    counter: i32,
    jobs_by_id: HashMap<i32, Arc<Job>>,
    jobs_by_client_working: HashMap<SocketAddr, Arc<Job>>,
    jobs_by_queue: HashMap<String, BinaryHeap<Arc<Job>>>,
}

/// All struct methods happen under a lock, so invariants are maintained across our lookup caches
impl Context {
    /// Add a new pending job to a queue
    fn add_pending_job(&self, data: Value, pri: u32, queue: String) -> i32 {
        let mut this = self.inner.lock().unwrap();

        this.counter += 1;
        let id = this.counter;
        let job = Arc::new(Job {
            id,
            pri,
            data,
            queue: queue.clone(),
        });

        this.jobs_by_id.insert(id, job.clone());
        match this.jobs_by_queue.get_mut(&queue) {
            Some(heap) => {
                heap.push(job);
            }
            None => {
                let mut heap = BinaryHeap::new();
                heap.push(job);
                this.jobs_by_queue.insert(queue, heap);
            }
        }

        id
    }

    /// Request a job from any of the given queues
    /// Returns job if we were able to pop, else none
    ///
    /// Implementation: We walk the jobs by queue and pop the one with the maximum priority
    /// The popped job is added to the client's working queue.
    fn poll_job(&self, queues: &Vec<String>, client: SocketAddr) -> Option<Arc<Job>> {
        let mut this = self.inner.lock().unwrap();

        // TODO: Use iterator on `queues` with `filter_map` to self.jobs_by_queue and a max_by
        let mut highest_priority_job: Option<Arc<Job>> = None;
        for queue_name in queues {
            if let Some(heap) = this.jobs_by_queue.get(queue_name) {
                if let Some(job) = heap.peek() {
                    if highest_priority_job.is_none()
                        || job > &highest_priority_job.as_ref().unwrap()
                    {
                        highest_priority_job = Some(job.clone());
                    }
                }
            }
        }

        if let Some(job) = highest_priority_job {
            // Move job to the client's working queue
            let heap = this.jobs_by_queue.get_mut(&job.queue).unwrap();
            heap.pop();
            this.jobs_by_client_working.insert(client, job.clone());

            Some(job)
        } else {
            None
        }
    }

    /// Try abort a job by ID
    /// Fails if the job is invalid, or the client is not working on it
    fn try_abort(&self, job_id: i32, client: &SocketAddr) -> Result<(), ()> {
        let mut this = self.inner.lock().unwrap();

        if let Some(job) = this.jobs_by_client_working.get(client) {
            if job.id == job_id {
                let job = this.jobs_by_client_working.remove(client).unwrap();
                if let Some(queue) = this.jobs_by_queue.get_mut(&job.queue) {
                    queue.push(job);
                }
                return Ok(());
            }
        }

        Err(())
    }

    /// Abort any jobs the client is working on
    fn abort_any(&self, client: &SocketAddr) {
        let mut this = self.inner.lock().unwrap();

        if let Some(job) = this.jobs_by_client_working.remove(client) {
            if let Some(queue) = this.jobs_by_queue.get_mut(&job.queue) {
                queue.push(job);
            }
        }
    }

    /// Try delete a job by ID
    /// Fails if the job is invalid
    fn try_delete(&self, job_id: i32) -> Result<(), ()> {
        let mut this = self.inner.lock().unwrap();

        let job = match this.jobs_by_id.remove(&job_id) {
            Some(job) => job,
            None => return Err(()),
        };

        let client_id = this
            .jobs_by_client_working
            .iter()
            .find(|(_, v)| v.id == job_id)
            .map(|(k, _)| k)
            .cloned();
        if let Some(k) = client_id {
            this.jobs_by_client_working.remove(&k);
            return Ok(());
        }

        if let Some(queue) = this.jobs_by_queue.get_mut(&job.queue) {
            // Need to rebuild the BinaryHeap, as we cannot remove arbitrary elements
            *queue = queue
                .drain()
                .filter(|j| Arc::ptr_eq(&job, j) == false)
                .collect();
            return Ok(());
        }

        unreachable!()
    }
}

async fn start_server() -> std::io::Result<()> {
    let listener = Async::<TcpListener>::bind(([0, 0, 0, 0], 7128))?;
    println!("Listening on port {}", listener.get_ref().local_addr()?);

    let ctx = Context::default();

    loop {
        let (stream, addr) = listener.accept().await?;
        let _ctx = ctx.clone();
        smol::spawn(async move {
            if let Err(e) = handle(stream, addr, _ctx).await {
                eprintln!("Error handling stream: {:?}", e);
            }
        })
        .detach();
    }
}
fn main() {
    smol::block_on(async {
        if let Err(e) = start_server().await {
            eprintln!("Error! {:?}", e);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_malformed_request() {}
}
