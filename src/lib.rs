/**
 * Meows is a simpmle library for making it easy to implement websocket message
 * handlers, built on top of async-tungstenite and the async ecosystem
 */

#[macro_use]
extern crate lazy_static;
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
use std::pin::Pin;
use std::sync::{Arc, Mutex};


/** Re-exporting for convenience */
pub use tungstenite::Message;
pub use serde_json::Value;

type DefaultDispatchFn = Arc<dyn Fn(String) -> BoxFuture<'static, Option<Message>> + Send + Sync>;
type DispatchFn = Arc<dyn Fn(Value) -> BoxFuture<'static, Option<Message>> + Send + Sync>;


/**
 * The internal mechanism for keeping track of message handlers
 */
pub struct Registry {
    dispatchers: HashMap<String, DispatchFn>,
    default: Option<DefaultDispatchFn>,
}
impl Registry {
    /**
     * Insert a handler into the registry
     */
    pub fn insert(&mut self, key: String, value: DispatchFn) {
        self.dispatchers.insert(key, value);
    }
    /**
     * Retrieve a registered handler
     */
    pub fn get(&self, key: &String) -> Option<&DispatchFn> {
        self.dispatchers.get(key)
    }
    /**
     * Set the default dispatch handler
     */
    pub fn set_default(&mut self, handler: DefaultDispatchFn) {
        self.default = Some(handler);
    }
}
impl Default for Registry {
    fn default() -> Registry {
        Registry {
            dispatchers: HashMap::new(),
            default: None,
        }
    }
}

lazy_static! {
    pub static ref REGISTRY: Arc<Mutex<Registry>> = {
        Arc::new(Mutex::new(Registry::default()))
    };
}

#[allow(unused_macros)]
#[macro_export]
macro_rules! meows {
    ($($e:expr), * => $($t:ty), *) => {
        $(
            meows::REGISTRY.lock().expect("Failed to unlock meows registry")
                .insert($e.to_string(),
                    std::sync::Arc::new(|m| Box::pin(<$t>::handle(m)))
                );
        )*
    }
}

#[allow(unused_macros)]
#[macro_export]
macro_rules! default_meows {
    ($T:ty) => {
        meows::REGISTRY.lock().expect("Failed to unlock meows registry")
            .set_default(
                std::sync::Arc::new(
                    |m| Box::pin(<$T>::handle(m))
                    )
            );
    }
}

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
    ttype: String,
    value: serde_json::Value,
}


pub type AsyncMessage = Pin<Box<dyn Future<Output=Option<Message>> + Send>>;

pub trait Handler<MessageType: DeserializeOwned, Output> {
    fn invoke_with_value(value: serde_json::Value) -> Output {
        if let Ok(real) = serde_json::from_value::<MessageType>(value) {
            return Self::handle(Some(real));
        }
        Self::handle(None)
    }
    fn handle(message: Option<MessageType>) -> Output;
}

/**
 * The Server is the primary means of listening for messages
 */
pub struct Server<State> {
    state: State,
}
impl Server<()> {
    pub fn new() -> Self {
        Server {
            state: (),
        }
    }

    pub async fn serve(&self, listen_on: String) -> Result<(), std::io::Error> {
        debug!("Starting to listen on: {}", &listen_on);
        let listener = Async::<TcpListener>::bind(listen_on)?;

        loop {
            let (stream, _) = listener.accept().await?;

            match async_tungstenite::accept_async(stream).await {
                Ok(ws) => {
                    Task::spawn(async move {
                        Server::handle_connection(ws).await;
                    }).detach();
                },
                Err(e) => {
                    error!("Failed to process WebSocket handshake: {}", e);
                }
            }
        }
    }

    async fn handle_connection(mut stream: WebSocketStream<Async<TcpStream>>) -> Result<(), std::io::Error> {
        while let Some(raw) = stream.next().await {
            trace!("WebSocket message received: {:?}", raw);
            match raw {
                Ok(message) => {
                    let message = message.to_string();

                    if let Ok(envelope) = serde_json::from_str::<Envelope>(&message) {
                        debug!("Envelope deserialized: {:?}", envelope);

                        /*
                         * This messing around with the values inside fo the registry are necessary
                         * because the registry's mutexguard cannot be held across the awaits that
                         * are below
                         */
                        let handler = match REGISTRY.lock().unwrap().get(&envelope.ttype) {
                            Some(h) => Some(h.clone()),
                            None => None,
                        };

                        if handler.is_some() {
                            if let Some(response) = (handler.unwrap())(envelope.value).await {
                                stream.send(response).await;
                            }
                        }
                    }
                    else {
                        /*
                         * If we didn't have a specific handler, try to invoke the default handler
                         * if it exists
                         */
                        let default = match &REGISTRY.lock().unwrap().default {
                            Some(d) => Some(d.clone()),
                            None => None,
                        };

                        if default.is_some() {
                            if let Some(response) = (default.unwrap())(message).await {
                                stream.send(response).await;
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Error receiving message: {}", e);
                }
            }
        }
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
