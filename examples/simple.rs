/**
 * The simple server just handles a ping/pong message via a websocket
 */

extern crate futures;
#[macro_use]
extern crate meows;
extern crate pretty_env_logger;
#[macro_use]
extern crate serde_derive;

use meows::*;
use smol;

#[derive(Debug, Deserialize, Serialize)]
struct Ping {
    msg: String,
}
impl Ping {
    fn handle(real: Ping) -> Option<Message> {
        info!("Ping handler: {:?}", real);
        Some(Message::text("pong"))
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
    let server = meows::Server { };

    smol::run(async move {
        server.serve("127.0.0.1:8105".to_string()).await
    })
}
