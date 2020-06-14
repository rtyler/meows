# Message Exchange Over Web Sockets 🐈

This crate implements the simple pattern needed for implementing
service-to-service message exchange over WebSocket connections. Users of meows
can handle typed JSON message with the option for a default handler for any
un-typed messages.

Meows is built on top of link:https://github.com/stjepang/smol[smol] for async
websocket handling behavior. This makes Meows compatible with other smol-based
applications, including those using
link:https://github.com/async-rs/async-std[async-std] 1.6.0 or later.

All messages intended to be handled must be wrapped in a meows appropriate
envelope described below:

```json
{
    "type" : "ping",
    "value" : {
        "msg" : "Hey kitty kitty"
    }
}
```

Which will then be mapped to a struct such as:

```rust
#[derive(Debug, Deserialize, Serialize)]
struct Ping {
    msg: String,
}
```

In essence, anything that serde can deserialize, can be passed through meows.


## Example

There are examples in the `examples/` directory, here's a simple message handler:

```rust
use log::*;
use meows::*;
use smol;

async fn handle_ping(mut req: Request<()>) -> Option<Message> {
    if let Some(ping) = req.from_value::<Ping>() {
        info!("Ping received with message: {}", ping.msg);
    }
    Some(Message::text("pong"))
}

fn main() -> Result<(), std::io::Error> {
    info!("Starting simple ping/pong websocket server with meows");
    let mut server = meows::Server::new();
    server.on("ping", handle_ping);

    smol::run(async move {
        server.serve("127.0.0.1:8105".to_string()).await
    })
}
```

## Contributing

Meows is a typical Rust project, e.g. `cargo build` and `cargo test` will do
most everything you would need to do.

There aren't nearly enough tests, helping with that would certainly be
appreciated :)

## Licensing

This crate is licensed as GPL-3.0+, which roughly means that if you're
packaging up Meows and distributing a binary to an end-user, you must also make
the source of the Meows code available to that end-user. It is intentionally
_not_ AGPL licensed, which means if you incorporate Meows into server-side code
that is never distributed to end-users, then you do not need to make the code
available to the service's users.

