use std::{ops::ControlFlow, time::Duration};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() {
    let url = "ws://127.0.0.1:3000/";

    println!("Connecting to url");

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");

    println!("Connected to network!");

    let (_sender, mut receiver) = ws_stream.split();

    let _ = tokio::spawn(async move {
        while let Some(Ok(message)) = receiver.next().await {
            if process_message(message).is_break() {
                break;
            }
        }
    }).await;

    println!("Loop exited, program stopping...");
}

fn process_message(msg: Message) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t)=>{println!("Got Text data: {:?}",t);},
        Message::Binary(b) => {println!("Got Binary data: {:?}", b);},
        Message::Ping(v) => {println!("Got Ping: {:?}", v);}, 
        Message::Pong(v) => {println!("Got Pong: {:?}", v);}, 
        Message::Close(v) => {
            println!("Got Close: {:?}", v);
            return ControlFlow::Break(());
        }, 
        _ => {println!("Got invalid message data");}
    }

    ControlFlow::Continue(())
}
