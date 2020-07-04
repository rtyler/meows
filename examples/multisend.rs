/**
 * The multisend example demonstrates how the server can send multiple responses
 * for a single inbound message
 */

#[macro_use]
extern crate meows;
extern crate pretty_env_logger;
#[macro_use]
extern crate serde_derive;

use meows::*;
use smol;
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize)]
struct Ping {
    msg: String,
}

/**
 * Handle ping messages, just send a pong back
 */
async fn handle_ping(mut req: Request<(), ()>) -> Option<Message> {
    if let Some(ping) = req.from_value::<Ping>() {
        info!("Ping received with message: {}", ping.msg);

        for i in 1..5 {
            req.sink.send(
                Message::text(format!("pong {}", i))
            ).await;

        }
    }
    None
}

/**
 * The default handler for unknown strings is to do nothing,
 * this handler will instead send whatever message we get back to the client
 */
async fn default_echo(message: String, _state: Arc<()>) -> Option<Message> {
    info!("Default echo: {}", message);
    Some(Message::text(message))
}

/**
 * THe main is pretty simple, just fire up the Meows server and set the handlers
 */
fn main() -> Result<(), std::io::Error> {
    pretty_env_logger::init();

    println!("Starting simple ping/pong websocket server with meows");
    let mut server = meows::Server::new();

    server.default(default_echo);
    server.on("ping", handle_ping);

    smol::run(async move { server.serve("127.0.0.1:8105".to_string()).await })
}
