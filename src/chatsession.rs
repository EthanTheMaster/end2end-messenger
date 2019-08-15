use actix::prelude::*;
use actix_web_actors::ws;

use serde::{Serialize, Deserialize};

use crate::chatserver;
use crate::chatserver::ChatServer;

use std::time::{Instant, Duration};

//Module `chatsession.rs` holds the Actor given to each connecting WebSocket client
//and this module acts on behalf of the WebSocket client when interacting with the
//chat server Actor. Any messages sent to the session Actor from the server Actor
//gets rerouted to the WebSocket client

//Probe the client to see if it is still connected ... Give client 5 chances to send a heartbeat
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(6);

//Track the client's state with a state machine
//This prevents clients from sending packets in an incorrect order
#[derive(Message, Serialize)]
pub enum ClientState {
    INIT,
    CONNECTED,
    AWAITING_VALIDATION,
    //the string is the room id that the client has been validated to enter
    VALIDATED(String),
}

//Incoming peers need to validate themselves to enter room
#[derive(Message, Serialize, Deserialize)]
pub struct ValidationRequest {
    pub room_id: String,
    pub id: String,
    //Validation string is interpreted however the clients in the room see fit
    //No particular protocol is enforced
    pub validation: String,
    pub accept: bool,
}

//This is a message sent by the server Actor to signify that a peer has
//sent a message in the room. This message should be dispatched to every
//client in the room.
#[derive(Message, Serialize)]
pub struct Text {
    pub id: String,
    pub message: String,
}

//Enumerates the valid packets that the client may send
#[derive(Serialize, Deserialize)]
pub enum ClientPacket {
    //This packet is sent by the client to enter/attempt to enter a room
    Register {
        //A peer can talk to another peer by registering into the same room via some string id
        room_id: String,
        //The validation field should be the HMAC of the client's id under some secret key unknown to server but
        //known to peers ... peers in the room will validate the incoming peer
        validation: String
    },
    //This packet is sent both by the client and the session Actor
    //The session Actor sends this packet as opposed to the struct above as
    //serde-json serialize enums with a tag making parsing easier for the WebSocket client
    ValidationRequest {
        room_id: String,
        id: String,
        validation: String,
        accept: bool,
    },
    //This packet is sent by the WebSocket client to dispatch a message across the room
    Text {
        message: String,
    },
    HEARTBEAT(String),
}

//Every connected user will have a session
pub struct ChatSession {
    pub id: String,
    pub server_addr: Addr<ChatServer>,
    pub state: ClientState,
    pub last_heartbeat: Instant,
}

//Make the ChatSession an Actor object
impl Actor for ChatSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        //Give the client its ID for verification stage
        ctx.text(format!("{}", self.id));
        self.state = ClientState::CONNECTED;

        //Register a heart beat monitor
        ctx.run_interval(HEARTBEAT_INTERVAL, |actor, context| {
            //Ping the client with a heartbeat message
            context.text(serde_json::to_string(&ClientPacket::HEARTBEAT("Ping".to_string())).unwrap());
            if Instant::now().duration_since(actor.last_heartbeat) > CLIENT_TIMEOUT {
                //No heartbeat ... disconnect the client
                context.stop();
            }
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        self.server_addr.do_send(chatserver::Disconnect {
            id: self.id.clone(),
            full_disconnect: true,
        });
    }
}

//Handles situation when the server contacts with a new message
impl Handler<Text> for ChatSession {
    type Result = ();

    fn handle(&mut self, msg: Text, ctx: &mut Self::Context) {
        println!("{}", serde_json::to_string(&msg).unwrap());
        ctx.text(serde_json::to_string(&msg).unwrap());
    }
}

//Server will send a message to update the client's state
impl Handler<ClientState> for ChatSession {
    type Result = ();

    fn handle(&mut self, msg: ClientState, ctx: &mut Self::Context) -> Self::Result {
        self.state = msg;
        ctx.text(serde_json::to_string(&self.state).unwrap());
    }
}

//Server wants to send a validation request for approval
impl Handler<ValidationRequest> for ChatSession {
    type Result = ();

    fn handle(&mut self, msg: ValidationRequest, ctx: &mut Self::Context) -> Self::Result {
        //Notify the client that a peer has connected and is awaiting validation
        //Verify that the current client is validated ... unvalidated people should not be able
        //to validate others
        if let ClientState::VALIDATED(_) = self.state {
            ctx.text(serde_json::to_string(&ClientPacket::ValidationRequest {
                room_id: msg.room_id,
                id: msg.id,
                validation: msg.validation,
                accept: false
            }).unwrap());
        }
    }
}

//Handle incoming messages from client
impl StreamHandler<ws::Message, ws::ProtocolError> for ChatSession {
    fn handle(&mut self, message: ws::Message, ctx: &mut Self::Context) {
        match message {
            ws::Message::Text(text) => {
                //Client is still alive ... update the heartbeat
                self.last_heartbeat = Instant::now();
                let res_packet = serde_json::from_str::<ClientPacket>(&text);
                //Pattern match on the packet
                match res_packet {
                    Ok(packet) => {
                        match packet {
                            //Contact server with registration request
                            ClientPacket::Register {room_id, validation} => {
                                self.server_addr.do_send(chatserver::Register {
                                    id: self.id.clone(),
                                    room_id,
                                    validation,
                                    addr: ctx.address()
                                })
                            },
                            //Client has sent a validation request ... redirect to server Actor
                            ClientPacket::ValidationRequest {  room_id, id, validation, accept } => {
                                match &self.state {
                                    ClientState::VALIDATED(validated_room_id) => {
                                        if validated_room_id != &room_id {
                                            ctx.text("Validation request cannot be given for another room.");
                                        } else {
                                            self.server_addr.do_send(ValidationRequest {
                                                room_id,
                                                id,
                                                validation,
                                                accept
                                            })
                                        }
                                    },
                                    _ => {
                                        ctx.text("Validation requests can only be sent by already validated clients.");
                                    }
                                }

                            },
                            ClientPacket::Text { message } => {
                                match &self.state {
                                    ClientState::VALIDATED(room_id) => {
                                        self.server_addr.do_send(chatserver::Message {
                                            id: self.id.clone(),
                                            room_id: room_id.clone(),
                                            message
                                        });
                                    },
                                    _ => {
                                        ctx.text("You must be validated into a room to sent a text message.")
                                    }
                                }
                            },
                            ClientPacket::HEARTBEAT(_) => {/*Do nothing ... heartbeat already registered*/}
                        }
                    },
                    Err(_) => {
                        ctx.text("Invalid Packet Sent");
                    },
                }
            },
            ws::Message::Binary(_bin) => {},
            ws::Message::Ping(_) => {ctx.text("PONG!")},
            ws::Message::Pong(_) => {},
            ws::Message::Close(_) => {},
            ws::Message::Nop => {},
        }
    }
}