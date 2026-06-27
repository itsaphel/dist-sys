use std::net::{TcpListener, TcpStream};
use smol::{io::{AsyncReadExt, AsyncWriteExt}, Async};

async fn handle(mut stream: Async<TcpStream>) -> std::io::Result<()> {
    // Read to EOF, then echo the results back
    println!("New connection.");
    let mut buf = vec![];
    let n = stream.read_to_end(&mut buf).await?;
    println!("Client finished writing {} bytes", n);
    let _ = stream.write(&buf).await?;
    Ok(())
}

async fn start_server() -> std::io::Result<()> {
    let listener = Async::<TcpListener>::bind(([127, 0, 0, 1], 7128))?;

    println!("Listening on port {}", listener.get_ref().local_addr()?);
    
    loop {
        // Accept a connection, handling it in a separate async task
        // `listener.accept` will block (or technically, return Pending) until one is available.
        let (stream, _) = listener.accept().await?;
        smol::spawn(async move {
            if let Err(e) = handle(stream).await {
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
