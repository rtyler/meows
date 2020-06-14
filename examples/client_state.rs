/**
 * This example demonstrates how to affiliate per-connection state with the
 * message handlers. This allows storing data associated with a single connection
 * between message invocations.
 */

#[macro_use]
extern crate meows;
extern crate pretty_env_logger;
#[macro_use]
extern crate serde_derive;

use meows::*;
use smol;

#[derive(Debug, Deserialize, Serialize)]
struct Greeting {
    name: String,
    email: Option<String>,
}

async fn handle_hello(mut req: Request<(), ClientState>) -> Option<Message> {
    let greeting: Greeting = req.from_value().expect("Failed to get greeting");

    if let Ok(mut cs) = req.client_state.write() {
        cs.name = Some(greeting.name);
        return Some(Message::text(format!("Why hello there {}", cs.name.as_ref().unwrap())));
    }

    None
}

#[derive(Debug, Deserialize, Serialize)]
struct Ping {
    msg: String,
}

async fn handle_ping(mut req: Request<(), ClientState>) -> Option<Message> {
    if let Some(ping) = req.from_value::<Ping>() {
        info!("Ping received with message: {}", ping.msg);
    }

    if let Ok(cs) = req.client_state.read() {
        if let Some(name) = &cs.name {
            info!("According to our client state, the name is: {}", name);
            return Some(Message::text(format!("pong for {}", name)));
        }
        else {
            warn!("No name has yet been sent! Give us a greeting yo!");
        }
    }
    Some(Message::text("pong"))
}


struct ClientState {
    /// Name of the person sending messages
    name: Option<String>,
}
impl ClientState {
}

impl Default for ClientState {
    fn default() -> Self {
        Self {
            name: None,
        }
    }
}


fn main() -> Result<(), std::io::Error> {
    pretty_env_logger::init();

    println!("Starting simple ping/pong websocket server with meows");
    let mut server = meows::Server::<(), ClientState>::with_state(());

    server.on("hello", handle_hello);
    server.on("ping", handle_ping);

    smol::run(async move { server.serve("127.0.0.1:8105".to_string()).await })
}
