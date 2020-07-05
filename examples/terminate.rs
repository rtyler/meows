/**
 * This example demonstrates terminating the server from another task or thread
 * inside the same process
 */

#[macro_use]
extern crate meows;
extern crate pretty_env_logger;

use futures::sink::SinkExt;
use meows::*;
use smol;
use std::time::Duration;
use std::sync::Arc;

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
    let mut controller = server.get_control_channel();

    smol::Task::spawn(async move {
        info!("Counting to three, then you have to go to your room mister!");
        for _ in 1..10 {
            let dur = Duration::from_secs(1);
            smol::Timer::after(dur).await;
        }
        controller.send(Control::Terminate).await;
    }).detach();

    smol::run(async move { server.serve("127.0.0.1:8105".to_string()).await })
}
