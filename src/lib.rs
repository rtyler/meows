/**
 * Meows is a simpmle library for making it easy to implement websocket message
 * handlers, built on top of async-tungstenite and the async ecosystem
 */

#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate smol;

use async_tungstenite::WebSocketStream;
use futures::future::BoxFuture;
use futures::prelude::*;
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
pub struct Request<State> {
    pub env: Envelope,
    pub state: Arc<State>,
}

impl<State> Request<State> {
    pub fn from_value<ValueType: DeserializeOwned>(&mut self) -> Option<ValueType> {
        serde_json::from_value(self.env.value.take()).map_or(None, |v| Some(v))
    }
}

/**
 * Endpoint comes from tide, and I'm still not sure how this magic works
 */
pub trait Endpoint<State>: Send + Sync + 'static {
    /// Invoke the endpoint within the given context
    fn call<'a>(&'a self, req: Request<State>) -> BoxFuture<'a, Option<Message>>;
}

impl<State, F: Send + Sync + 'static, Fut> Endpoint<State> for F
where
    F: Fn(Request<State>) -> Fut,
    Fut: Future<Output = Option<Message>> + Send + 'static,
{
    fn call<'a>(&'a self, req: Request<State>) -> BoxFuture<'a, Option<Message>> {
        let fut = (self)(req);
        Box::pin(fut)
    }
}

pub trait DefaultEndpoint<State>: Send + Sync + 'static {
    /// Invoke the endpoint within the given context
    fn call<'a>(&'a self, msg: String, state: Arc<State>) -> BoxFuture<'a, Option<Message>>;
}

impl<State, F: Send + Sync + 'static, Fut> DefaultEndpoint<State> for F
where
    F: Fn(String, Arc<State>) -> Fut,
    Fut: Future<Output = Option<Message>> + Send + 'static,
{
    fn call<'a>(&'a self, msg: String, state: Arc<State>) -> BoxFuture<'a, Option<Message>> {
        let fut = (self)(msg, state);
        Box::pin(fut)
    }
}

type Callback<State> = Arc<Box<dyn Endpoint<State>>>;
type DefaultCallback<State> = Arc<Box<dyn DefaultEndpoint<State>>>;

/**
 * The Server is the primary means of listening for messages
 */
pub struct Server<State> {
    state: Arc<State>,
    handlers: Arc<RwLock<HashMap<String, Callback<State>>>>,
    default: DefaultCallback<State>,
}

impl<State: 'static + Send + Sync> Server<State> {
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
     * async fn handle_ping(mut req: Request<()>) -> Option<Message> {
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
    pub fn on(&mut self, message_type: &str, invoke: impl Endpoint<State>) {
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
    pub fn default(&mut self, invoke: impl DefaultEndpoint<State>) {
        self.default = Arc::new(Box::new(invoke));
    }

    /**
     * Default handler which is used if the user doesn't specify a handler
     * that should be used for messages Meows doesn't understand
     */
    async fn default_handler(_msg: String, _state: Arc<State>) -> Option<Message> {
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
     *   });
     *   Ok(())
     * }
     * ```
     */
    pub async fn serve(&self, listen_on: String) -> Result<(), std::io::Error> {
        debug!("Starting to listen on: {}", &listen_on);
        let listener = Async::<TcpListener>::bind(listen_on)?;

        loop {
            let (stream, _) = listener.accept().await?;

            match async_tungstenite::accept_async(stream).await {
                Ok(ws) => {
                    let state = self.state.clone();
                    let handlers = self.handlers.clone();
                    let default = self.default.clone();
                    Task::spawn(async move {
                        Server::<State>::handle_connection(state, default, handlers, ws).await;
                    })
                    .detach();
                }
                Err(e) => {
                    error!("Failed to process WebSocket handshake: {}", e);
                }
            }
        }
    }

    /**
     * Handle connection is invoked in its own task for each new WebSocket,
     * from which it will read messages and invoke the appropriate handlers
     */
    async fn handle_connection(
        state: Arc<State>,
        default: DefaultCallback<State>,
        handlers: Arc<RwLock<HashMap<String, Callback<State>>>>,
        mut stream: WebSocketStream<Async<TcpStream>>,
    ) -> Result<(), std::io::Error> {
        while let Some(raw) = stream.next().await {
            trace!("WebSocket message received: {:?}", raw);
            match raw {
                Ok(message) => {
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
                            };

                            if let Some(response) = handler.call(req).await {
                                stream.send(response).await;
                            }
                        }
                    } else {
                        if let Some(response) = default.call(message, state.clone()).await {
                            stream.send(response).await;
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

impl Server<()> {
    pub fn new() -> Self {
        Server {
            state: Arc::new(()),
            handlers: Arc::new(RwLock::new(HashMap::default())),
            default: Arc::new(Box::new(Server::<()>::default_handler)),
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
