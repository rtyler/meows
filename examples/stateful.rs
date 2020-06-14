/**
 * The stateful server carries server and connection state to websocket handlers
 */

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

struct ServerState {
    secret: String,
}

async fn handle_ping(mut req: Request<ServerState, ()>) -> Option<Message> {
    info!("My state secret is: {}", req.state.secret);
    if let Some(ping) = req.from_value::<Ping>() {
        info!("Ping received with message: {}", ping.msg);
    }
    Some(Message::text("pong"))
}

fn main() -> Result<(), std::io::Error> {
    pretty_env_logger::init();

    let state = ServerState {
        secret: String::from("hunter2"),
    };

    println!("Starting simple ping/pong websocket server with meows");
    let mut server = meows::Server::with_state(state);
    server.on("ping", handle_ping);

    smol::run(async move { server.serve("127.0.0.1:8105".to_string()).await })
}
