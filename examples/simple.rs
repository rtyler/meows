/**
 * The simple server just handles a ping/pong message via a websocket
 */

extern crate futures;
#[macro_use]
extern crate meows;
extern crate pretty_env_logger;
#[macro_use]
extern crate serde_derive;

use async_tungstenite::WebSocketStream;
use meows::*;
use smol;

#[derive(Debug, Deserialize, Serialize)]
struct Ping {
    msg: String,
}
impl meows::Handler for Ping {
    fn handle(real: Ping) -> Result<(), std::io::Error> {
        info!("Ping handler: {:?}", real);
        Ok(())
    }
}

struct Echo;
impl Echo {
    async fn handle(message: String) -> Option<tungstenite::Message> {
        info!("Message received! {}", message);
        Some(tungstenite::Message::text(message))
    }
}

fn main() -> Result<(), std::io::Error> {
    pretty_env_logger::init();
    default_meows!(Echo);
    meows!("ping" => Ping);

    println!("Starting simple ping/pong websocket server with meows");
    let server = meows::Server { };

    smol::run(async move {
        server.serve("127.0.0.1:8105".to_string()).await
    })
}
