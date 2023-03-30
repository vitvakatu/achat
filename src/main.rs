use anyhow::anyhow;
use anyhow::Result;
use async_std::{
    io::BufReader,
    net::{TcpListener, TcpStream, ToSocketAddrs},
    prelude::*,
    task,
};
use futures::channel::mpsc;
use futures::sink::SinkExt;
use std::{
    collections::hash_map::{Entry, HashMap},
    sync::Arc,
};
type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

// 1. Proper async stream handling
// 2. `connection_writer_loop` implementation
// -- Check how it works
// 3. Add error handling
// 4. Clean shutdown
// 5. Handle client disconnects (optional)

fn main() -> Result<()> {
    task::block_on(accept_loop("127.0.0.1:8080"))
}

async fn accept_loop(addr: impl ToSocketAddrs) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;

    let (broker_sender, broker_receiver) = mpsc::unbounded();
    let _broker_handle = task::spawn(broker_loop(broker_receiver));
    // 1
    for stream in listener.incoming() {
        println!("Accepting from: {}", stream.peer_addr()?);
        task::spawn(connection_loop(broker_sender.clone(), stream)); // 3
    }
    // 4
    Ok(())
}

async fn connection_loop(mut broker: Sender<Event>, stream: TcpStream) -> Result<()> {
    let stream = Arc::new(stream);
    let reader = BufReader::new(&*stream);
    let mut lines = reader.lines();
    // 5

    let name = match lines.next().await {
        None => Err(anyhow!("peer disconnected immediately"))?,
        Some(line) => line?,
    };
    broker
        .send(Event::NewPeer {
            name: name.clone(),
            stream: Arc::clone(&stream),
        })
        .await
        .unwrap();

    while let Some(line) = lines.next().await {
        let line = line?;
        let (dest, msg) = match line.find(':') {
            None => continue,
            Some(idx) => (&line[..idx], line[idx + 1..].trim()),
        };
        let dest: Vec<String> = dest
            .split(',')
            .map(|name| name.trim().to_string())
            .collect();
        let msg: String = msg.to_string();

        broker
            .send(Event::Message {
                from: name.clone(),
                to: dest,
                msg,
            })
            .await
            .unwrap();
    }
    Ok(())
}

// 5
async fn connection_writer_loop(messages: Receiver<String>, stream: Arc<TcpStream>) -> Result<()> {
    todo!(); // 2
    Ok(())
}

#[derive(Debug)]
enum Event {
    NewPeer {
        name: String,
        stream: Arc<TcpStream>,
    },
    Message {
        from: String,
        to: Vec<String>,
        msg: String,
    },
}

async fn broker_loop(mut events: Receiver<Event>) -> Result<()> {
    let mut peers: HashMap<String, Sender<String>> = HashMap::new();
    // 5

    while let Some(event) = events.next().await {
        match event {
            Event::Message { from, to, msg } => {
                for addr in to {
                    if let Some(peer) = peers.get_mut(&addr) {
                        let msg = format!("from {}: {}\n", from, msg);
                        peer.send(msg).await?
                    }
                }
            }
            Event::NewPeer { name, stream } => match peers.entry(name) {
                Entry::Occupied(..) => (),
                Entry::Vacant(entry) => {
                    let (client_sender, client_receiver) = mpsc::unbounded();
                    entry.insert(client_sender);
                    task::spawn(connection_writer_loop(client_receiver, stream));
                    // 3
                }
            },
        }
    }
    // 4
    Ok(())
}
