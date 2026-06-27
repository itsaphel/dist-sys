use std::net::{SocketAddr, TcpListener, TcpStream};
use anyhow::bail;
use serde::{Deserialize, Serialize};
use smol::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, Async};

#[derive(Deserialize)]
struct Request {
    method: String,
    number: f64,
}

#[derive(Serialize)]
struct Response {
    method: String,
    prime: bool
}

impl Response {
    fn legal(is_prime: bool) -> Result<String, serde_json::Error> {
        let resp = Response {
            method: String::from("isPrime"),
            prime: is_prime,
        };
        Ok(serde_json::to_string(&resp)? + "\n")
    }
    
    fn illegal() -> String {
        String::from("illegal\n")
    }
}

fn is_prime(n: u64) -> bool {
    if n <= 1 {
        return false;
    }
    if n <= 3 {
        return true;
    }
    if n % 2 == 0 || n % 3 == 0 {
        return false;
    }
    
    let sqrt_n = (n as f64).sqrt() as u64 + 1;
    let mut i = 5;
    while i <= sqrt_n {
        if n % i == 0 || n % (i + 2) == 0 {
            return false;
        }
        i += 6;
    }
    true
}

/// Get number in request
fn get_number_from_request(line: String) -> anyhow::Result<f64> {
    let request: Request = serde_json::from_str(&line)?;
    
    if request.method != "isPrime" {
        bail!("No or incorrect request method");
    }
    
    Ok(request.number)
}

async fn handle(stream: Async<TcpStream>, addr: SocketAddr) -> anyhow::Result<()> {
    println!("Received new connection.");
    
    let (reader, mut writer) = smol::io::split(stream);
    let mut reader = BufReader::new(reader);
    
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            println!("[{}] Connection closed by client", addr);
            return Ok(());
        }
        println!("[{}] Request: {}", addr, line);
        
        match get_number_from_request(line) {
            Ok(n) => {
                let resp = if n.fract() == 0.0 {
                    Response::legal(is_prime(n as u64))?
                } else {
                    Response::legal(false)?
                };
                println!("[{}] Writing response {}", addr, resp);
                writer.write_all(resp.as_bytes()).await?;
            },
            Err(_) => {
                let resp = Response::illegal();
                println!("[{}] Writing ERROR {}", addr, resp);
                writer.write_all(resp.as_bytes()).await?;
                writer.close().await?;
                return Ok(())
            },
        }
    }
}

async fn start_server() -> std::io::Result<()> {
    let listener = Async::<TcpListener>::bind(([0, 0, 0, 0], 7128))?;

    println!("Listening on port {}", listener.get_ref().local_addr()?);

    loop {
        // Accept a connection, handling it in a separate async task
        // `listener.accept` will block (or technically, return Pending) until one is available.
        let (stream, addr) = listener.accept().await?;
        smol::spawn(async move {
            if let Err(e) = handle(stream, addr).await {
                eprintln!("Error handling stream: {:?}", e);
            }
        }).detach();
    }
}
fn main() {
    smol::block_on(async {
        if let Err(e) = start_server().await {
            eprintln!("Error! {:?}", e);
        }
    })
}

mod tests {
    
}