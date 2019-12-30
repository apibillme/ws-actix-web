use actix::*;
use actix_web_actors::ws;
use std::time::{Duration, Instant};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix::io::SinkWrite;
use actix_codec::{AsyncRead, AsyncWrite, Framed};
use awc::{
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
};
use bytes::Bytes;
use futures::stream::{SplitSink};

pub struct WSClient<T>(pub SinkWrite<Message, SplitSink<Framed<T, Codec>, Message>>)
where
    T: AsyncRead + AsyncWrite;

#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientCommand(pub String);

impl<T: 'static> Actor for WSClient<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // start heartbeats otherwise server will disconnect after 10 seconds
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
        println!("Disconnected");

        // Stop application on disconnect
        System::current().stop();
    }
}

impl<T: 'static> WSClient<T>
where
    T: AsyncRead + AsyncWrite,
{
    pub fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(1, 0), |act, ctx| {
            act.0.write(Message::Ping(Bytes::from_static(b""))).unwrap();
            act.hb(ctx);

            // client should also check for a timeout here, similar to the
            // server code
        });
    }
}

/// Handle stdin commands
impl<T: 'static> Handler<ClientCommand> for WSClient<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Result = ();

    fn handle(&mut self, msg: ClientCommand, _ctx: &mut Context<Self>) {
        self.0.write(Message::Text(msg.0)).unwrap();
    }
}

/// Handle server websocket messages
impl<T: 'static> StreamHandler<Result<Frame, WsProtocolError>> for WSClient<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn handle(&mut self, msg: Result<Frame, WsProtocolError>, _: &mut Context<Self>) {
        if let Ok(Frame::Text(txt)) = msg {
            println!("Server: {:?}", txt)
        }
    }

    fn started(&mut self, _ctx: &mut Context<Self>) {
        println!("Connected");
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        println!("Server disconnected");
        ctx.stop()
    }
}

impl<T: 'static> actix::io::WriteHandler<WsProtocolError> for WSClient<T> where
    T: AsyncRead + AsyncWrite
{
}

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// do websocket handshake and start `MyWebSocket` actor
pub async fn ws_index(r: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    ws::start(MyWebSocket::new(), &r, stream)
}

/// websocket connection is long running connection, it easier
/// to handle with an actor
struct MyWebSocket {
    /// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
    /// otherwise we drop connection.
    hb: Instant,
}

impl Actor for MyWebSocket {
    type Context = ws::WebsocketContext<Self>;

    /// Method is called on actor start. We start the heartbeat process here.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
    }
}

/// Handler for `ws::Message`
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWebSocket {
    fn handle(
        &mut self,
        msg: Result<ws::Message, ws::ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        // process websocket messages
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(ws::Message::Text(text)) => ctx.text(text),
            Ok(ws::Message::Binary(bin)) => ctx.binary(bin),
            Ok(ws::Message::Close(_)) => {
                ctx.stop();
            }
            _ => ctx.stop(),
        }
    }
}

impl MyWebSocket {
    fn new() -> Self {
        Self { hb: Instant::now() }
    }
    /// helper method that sends ping to client every second.
    ///
    /// also this method checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                // stop actor
                ctx.stop();

                // don't try to send a ping
                return;
            }
            ctx.ping(b"");
        });
    }
}
