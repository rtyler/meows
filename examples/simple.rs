/**
 * The simple server just handles a ping/pong message via a websocket
 */

extern crate futures;
#[macro_use]
extern crate meows;
extern crate pretty_env_logger;
#[macro_use]
extern crate serde_derive;

use std::future::Future;
use futures::future::FutureExt;
use std::pin::Pin;

use meows::*;
use smol;

#[derive(Debug, Deserialize, Serialize)]
struct Ping {
    msg: String,
}

async fn print_a_thing() {
    info!("printing thing");
}

async fn handle_ping(ping: Ping) -> Option<Message> {
    Some(Message::text("pong"))

}

struct PingHandler;
impl Handler<Ping, AsyncMessage> for PingHandler {
    fn handle(p: Option<Ping>) -> AsyncMessage {
        async move {
            if p.is_some() {
                info!("Ping handler: {:?}", p);
                print_a_thing().await;
            }
            else {
                error!("Did not receive a properly formatted ping message");
                None
            }
        }.boxed()
    }
}

/**
 * The Echo struct will act as the simple default handler for Meows which means
 * anything that cannot be parsed properly will just be echoed back
 */
struct Echo;
impl Echo {
    async fn handle(message: String) -> Option<Message> {
        info!("Message received! {}", message);
        Some(Message::text(message))
    }
}

fn main() -> Result<(), std::io::Error> {
    pretty_env_logger::init();
    default_meows!(Echo);
    meows!("ping" => Ping);

    println!("Starting simple ping/pong websocket server with meows");
    let server = meows::Server::new();

    //server.on("ping", handle_ping);

    smol::run(async move {
        server.serve("127.0.0.1:8105".to_string()).await
    })
}
