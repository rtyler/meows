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
use futures::stream::*;
use log::*;
use serde::de::DeserializeOwned;
use smol::{Async, Task};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

type DefaultDispatch = Arc<dyn Fn(String) -> (Box<dyn std::future::Future<Output=tungstenite::Message> + Unpin>) + Send + Sync>;

/**
 * The internal mechanism for keeping track of message handlers
 */
pub struct Registry {
    dispatchers: HashMap<String, Dispatch>,
    default: Option<DefaultDispatch>,
}
impl Registry {
    /**
     * Insert a handler into the registry
     */
    pub fn insert(&mut self, key: String, value: Dispatch) {
        self.dispatchers.insert(key, value);
    }
    /**
     * Retrieve a registered handler
     */
    pub fn get(&self, key: &String) -> Option<&Dispatch> {
        self.dispatchers.get(key)
    }
    /**
     * Set the default dispatch handler
     */
    pub fn set_default(&mut self, handler: DefaultDispatch) {
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
                .insert($e.to_string(), meows::Dispatch::new(|v| { <$t>::handle_value::<$t>(v); }));
        )*
    }
}

#[allow(unused_macros)]
#[macro_export]
macro_rules! default_meows {
    ($T:ty) => {
        meows::REGISTRY.lock().expect("Failed to unlock meows registry")
            .set_default(
                Arc::new(
                    |m| Box::new(<$T>::handle(m))
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

/**
 * The Handler trait must be applied to any structs which are expected to
 * be deserialized and instantiated to handle an incoming message
 *
 * For example
 *
 * ```
 *  # #[macro_use] extern crate serde_derive; fn main() {
 *  use meows::Handler;
 *
 *  #[derive(Debug, Deserialize, Serialize)]
 *  struct Hello {
 *      friend: String,
 *  }
 *  impl meows::Handler for Hello {
 *      fn handle(r: Self) -> Result<(), std::io::Error> {
 *          println!("Handling the hello for: {:?}", r);
 *          Ok(())
 *      }
 *  }
 *  let value = serde_json::from_str(r#"{"friend":"ferris"}"#).unwrap();
 *  assert!(Hello::handle_value::<Hello>(value).is_ok());
 *  # }
 * ```
 */
pub trait Handler: Send + Sync {
    /**
     * convert will take the given Value and attempt to convert it to the trait implementer's type
     *
     * If the conversion cannot be done properly, None will be returned
     */
    fn convert(v: serde_json::Value) -> Option<Self> where Self: std::marker::Sized + serde::de::DeserializeOwned {
        serde_json::from_value::<Self>(v).map_or(None, |res| Some(res))
    }

    /**
     * Implementors should implement the handle function which will be invoked
     * with a deserialized/constructed version of the struct itself
     */
    fn handle(r: Self) -> Result<(), std::io::Error>;

    /**
     * handle must be implemented by the trait implementer and should
     * do something novel with the value
     */
    fn handle_value<T: Handler + DeserializeOwned>(v: serde_json::Value) -> Result<(), std::io::Error> {
        if let Some(real) = T::convert(v) {
            return Handler::handle(real);
        }
        Err(std::io::Error::new(std::io::ErrorKind::Other, "oh no!"))
    }
}

/**
 * Dispatch is a struct which exists solely to help Box some closures into the
 * registry.
 */
pub struct Dispatch {
    f: Box<dyn Fn(serde_json::Value) + Send + Sync>,
}
impl Dispatch {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(serde_json::Value) + 'static + Send + Sync,
    {
        Self { f: Box::new(f) }
    }
}


/**
 * The Server is the primary means of listening for messages
 */
pub struct Server {
}
impl Server {
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

                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message) {
                        debug!("Value deserialized: {}", value);
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
                            (default.unwrap())(message).await;
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
