/*
 * This is apparently helpful/necessary for dealing with recursion wackiness
 * with the use of futures::select! inside of the `serve` function.
 *
 * This should be removed at a later point when somebody understands better than
 * I do currently, wth should happen.
 */
#![recursion_limit="256"]
/**
 * Meows is a simple library for making it easy to implement websocket message
 * handlers, built on top of async-tungstenite and the async ecosystem
 */

#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate smol;

use async_tungstenite::WebSocketStream;
use futures::future::BoxFuture;
use futures::channel::mpsc::{channel, Sender, Receiver};
use futures::prelude::*;
use futures::sink::SinkExt;
use log::*;
use serde::de::DeserializeOwned;
use smol::{Async, Task};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};

pub use serde_json::Value;
/** Re-exporting for convenience */
pub use tungstenite::Message;

/**
 * The Envelope handles the serialization/deserialization of the outer part of a
 * websocket message.
 *
 * All websocket messages are expected to have the basic format of:
 *  ```json
 *  {
 *      "type" : "foo",
 *      "value": {}
 *  }
 *  ```
 *  The contents of `value` can be completely arbitrary and are expected to be
 *  deserializable into whatever the `type` value string is , e.g. `Foo` in this
 *  example.
 */
#[derive(Debug, Deserialize, Serialize)]
pub struct Envelope {
    #[serde(rename = "type")]
    pub ttype: String,
    pub value: Value,
}

/**
 * Support converting an Envelope directly into a String type
 *
 * ```
 * use meows::Envelope;
 *
 * let e = Envelope { ttype: String::from("rust"), value: serde_json::Value::Null };
 * let into: String = e.into();
 * assert_eq!(r#"{"type":"rust","value":null}"#, into);
 * ```
 */
impl Into<String> for Envelope {
    fn into(self) -> String {
        serde_json::to_string(&self).expect("Curiosly failed to serialize an envelope")
    }
}

/**
 * THe Request struct brings message specific information into the handlers
 */
pub struct Request<ServerState, ClientState> {
    pub env: Envelope,
    pub state: Arc<ServerState>,
    pub client_state: Arc<RwLock<ClientState>>,
    pub sink: Sender<Message>,
}

impl<ServerState, ClientState> Request<ServerState, ClientState> {
    pub fn from_value<ValueType: DeserializeOwned>(&mut self) -> Option<ValueType> {
        serde_json::from_value(self.env.value.take()).map_or(None, |v| Some(v))
    }
}

/**
 * Endpoint comes from tide, and I'm still not sure how this magic works
 */
pub trait Endpoint<ServerState, ClientState>: Send + Sync + 'static {
    /// Invoke the endpoint within the given context
    fn call<'a>(&'a self, req: Request<ServerState, ClientState>) -> BoxFuture<'a, Option<Message>>;
}

impl<ServerState, ClientState, F: Send + Sync + 'static, Fut> Endpoint<ServerState, ClientState> for F
where
    F: Fn(Request<ServerState, ClientState>) -> Fut,
    Fut: Future<Output = Option<Message>> + Send + 'static,
{
    fn call<'a>(&'a self, req: Request<ServerState, ClientState>) -> BoxFuture<'a, Option<Message>> {
        let fut = (self)(req);
        Box::pin(fut)
    }
}

pub trait DefaultEndpoint<ServerState, ClientState>: Send + Sync + 'static {
    /// Invoke the endpoint within the given context
    fn call<'a>(&'a self, msg: String, state: Arc<ServerState>) -> BoxFuture<'a, Option<Message>>;
}

impl<ServerState, ClientState, F: Send + Sync + 'static, Fut> DefaultEndpoint<ServerState, ClientState> for F
where
    F: Fn(String, Arc<ServerState>) -> Fut,
    Fut: Future<Output = Option<Message>> + Send + 'static,
{
    fn call<'a>(&'a self, msg: String, state: Arc<ServerState>) -> BoxFuture<'a, Option<Message>> {
        let fut = (self)(msg, state);
        Box::pin(fut)
    }
}

type Callback<ServerState, ClientState> = Arc<Box<dyn Endpoint<ServerState, ClientState>>>;
type DefaultCallback<ServerState, ClientState> = Arc<Box<dyn DefaultEndpoint<ServerState, ClientState>>>;

/**
 * The Control enum contains all the control commands that another task/thread
 * can send to the meows::Server in order to control some of its behaviors
 */
#[derive(Debug)]
pub enum Control {
    // Terminate the server loop
    Terminate,
}


#[derive(Debug)]
struct Controller {
    tx: Sender<Control>,
    rx: Receiver<Control>,
}

impl Default for Controller {
    fn default() -> Self {
        let (tx, rx) = channel::<Control>(1);
        Self {
            tx,
            rx,
        }
    }
}

/**
 * The Server is the primary means of listening for messages
 */
pub struct Server<ServerState, ClientState> {
    control: Controller,
    state: Arc<ServerState>,
    handlers: Arc<RwLock<HashMap<String, Callback<ServerState, ClientState>>>>,
    default: DefaultCallback<ServerState, ClientState>,
}

impl<ServerState: 'static + Send + Sync, ClientState: 'static + Default + Send + Sync> Server<ServerState, ClientState> {

    /**
     * with_state will construct the Server with the given state object
     */
    pub fn with_state(state: ServerState) -> Self {
        Server {
            control: Controller::default(),
            state: Arc::new(state),
            handlers: Arc::new(RwLock::new(HashMap::default())),
            default: Arc::new(Box::new(Server::<ServerState, ClientState>::default_handler)),
        }
    }

    /**
     * Add a handler for a specific message type
     *
     * ```
     * use meows::*;
     * #[macro_use]
     * extern crate serde_derive;
     *
     * #[derive(Debug, Deserialize, Serialize)]
     * struct Ping {
     *     msg: String,
     * }
     *
     * async fn handle_ping(mut req: Request<(), ()>) -> Option<Message> {
     *   if let Some(ping) = req.from_value::<Ping>() {
     *       println!("Ping received: {:?}", ping);
     *   }
     *   Some(Message::text("pong"))
     * }
     *
     * # fn main() {
     * let mut server = Server::new();
     * server.on("ping", handle_ping);
     * # }
     * ```
     */
    pub fn on(&mut self, message_type: &str, invoke: impl Endpoint<ServerState, ClientState>) {
        if let Ok(mut h) = self.handlers.write() {
            h.insert(message_type.to_owned(), Arc::new(Box::new(invoke)));
        }
    }

    /**
     * Set the default message handler, which will be invoked any time that a
     * message is received that cannot be deserialized as a Meows Envelope
     *
     * ```
     * use meows::*;
     * use std::sync::Arc;
     *
     * async fn my_default(message: String, _state: Arc<()>) -> Option<Message> {
     *   None
     * }
     *
     * let mut server = Server::new();
     * server.default(my_default);
     * ```
     */
    pub fn default(&mut self, invoke: impl DefaultEndpoint<ServerState, ClientState>) {
        self.default = Arc::new(Box::new(invoke));
    }

    /**
     * Retrieve a cloned reference to the contrroller's sender
     *
     * This allows the caller to send messages to the control loop from other
     * tasks and threads
     */
    pub fn get_control_channel(&self) -> Sender<Control> {
        return self.control.tx.clone();
    }

    /**
     * Default handler which is used if the user doesn't specify a handler
     * that should be used for messages Meows doesn't understand
     */
    async fn default_handler(_msg: String, _state: Arc<ServerState>) -> Option<Message> {
        None
    }

    /**
     * The serve() function will listen for inbound webhook connections
     *
     *
     * ```no_run
     * use meows::*;
     * use smol;
     *
     * fn main() -> Result<(), std::io::Error> {
     *   let mut server = Server::new();
     *   smol::run(async move {
     *     server.serve("127.0.0.1:8105".to_string()).await
     *   })
     * }
     * ```
     */
    pub async fn serve(&mut self, listen_on: String) -> Result<(), std::io::Error> {
        debug!("Starting to listen on: {}", &listen_on);
        let listener = Async::<TcpListener>::bind(listen_on)?;

        loop {
            // Beware the recursion issues here, see:
            // <https://stackoverflow.com/questions/56930108/select-from-a-list-of-sockets-using-futures>
            futures::select! {
                res = listener.accept().fuse() => {
                    let (stream, _) = res?;
                    match async_tungstenite::accept_async(stream).await {
                        Ok(ws) => {
                            let state = self.state.clone();
                            let handlers = self.handlers.clone();
                            let default = self.default.clone();
                            Task::spawn(async move {
                                Server::<ServerState, ClientState>::handle_connection(state, default, handlers, ws)
                                    .await;
                            })
                            .detach();
                        },
                        Err(e) => {
                            error!("Failed to process WebSocket handshake: {}", e);
                        },
                    }
                },
                // XXX: This is ending up dominating the loop, need to figure out a way to
                // try-recv() in a block like this
                ctrl = self.control.rx.next().fuse() => {
                    info!("receiving control socket: {:?}", ctrl);
                    match ctrl {
                        Some(Control::Terminate) => {
                            // Exit out of our accept loop
                            return Ok(());
                        },
                        other => {
                            warn!("Unhandled message on control channel: {:?}", other);
                        }
                    }
                },
            }
        }
    }

    /**
     * Handle connection is invoked in its own task for each new WebSocket,
     * from which it will read messages and invoke the appropriate handlers
     */
    async fn handle_connection(
        state: Arc<ServerState>,
        default: DefaultCallback<ServerState, ClientState>,
        handlers: Arc<RwLock<HashMap<String, Callback<ServerState, ClientState>>>>,
        stream: WebSocketStream<Async<TcpStream>>,
    ) -> Result<(), std::io::Error> {

        let client_state = Arc::new(RwLock::new(ClientState::default()));

        /*
         * The WebSocketStream must be split into the reader and writer since
         * Meows intentionally decouples reading from writing.
         *
         * This allows for passing a handler an channel sink with which it can
         * send _multiple_ messages, not just the Message returned by the handler
         * function itself.
         */
        let (mut writer, mut reader) = stream.split();
        let (mut channel_tx, mut channel_rx) = channel::<Message>(1024);

        Task::spawn(async move {
            while let Some(outgoing) = channel_rx.next().await {
                trace!("Must send {:?} to the socket", outgoing);
                writer.send(outgoing).await;
            }
            trace!("Writer task closing for a socket");
        }).detach();

        while let Some(raw) = reader.next().await {
            let client_state = client_state.clone();

            trace!("WebSocket message received: {:?}", raw);
            match raw {
                Ok(message) => {
                    if message.is_close() {
                        debug!("The client has closed the connection, gracefully meowing out");
                        break;
                    }
                    let message = message.to_string();

                    if let Ok(envelope) = serde_json::from_str::<Envelope>(&message) {
                        debug!("Envelope deserialized: {:?}", envelope);

                        let handler = match handlers.read() {
                            Ok(h) => {
                                if let Some(handler) = h.get(&envelope.ttype) {
                                    Some(handler.clone())
                                } else {
                                    debug!("No handler found for message type: {}", envelope.ttype);
                                    None
                                }
                            }
                            _ => None,
                        };

                        if let Some(handler) = handler {
                            let req = Request {
                                env: envelope,
                                state: state.clone(),
                                client_state: client_state.clone(),
                                sink: channel_tx.clone(),
                            };

                            if let Some(response) = handler.call(req).await {
                                channel_tx.send(response).await;
                            }
                        }
                    } else {
                        if let Some(response) = default.call(message, state.clone()).await {
                            channel_tx.send(response).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving message: {}", e);
                }
            }
        }
        Ok(())
    }
}

impl Server<(), ()> {
    pub fn new() -> Self {
        Server {
            control: Controller::default(),
            state: Arc::new(()),
            handlers: Arc::new(RwLock::new(HashMap::default())),
            default: Arc::new(Box::new(Server::<(), ()>::default_handler)),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
