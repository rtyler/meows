/**
 * The simple server just handles a ping/pong message via a websocket
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

async fn handle_ping(mut req: Request<()>) -> Option<Message> {
    if let Some(ping) = req.from_value::<Ping>() {
        info!("Ping received: {:?}", ping);
    }
    Some(Message::text("pong"))
}

async fn default_echo(message: String, _state: Arc<()>) -> Option<Message> {
    info!("Default echo: {}", message);
    Some(Message::text(message))
}

fn main() -> Result<(), std::io::Error> {
    pretty_env_logger::init();

    println!("Starting simple ping/pong websocket server with meows");
    let mut server = meows::Server::new();

    server.default(default_echo);
    server.on("ping", handle_ping);

    smol::run(async move {
        server.serve("127.0.0.1:8105".to_string()).await
    })
}
