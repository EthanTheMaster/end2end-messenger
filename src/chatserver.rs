extern crate actix;

use actix::prelude::*;
use crate::ChatSession;
use std::collections::{HashMap, HashSet};

use crate::chatsession::ClientState;
use crate::chatsession::ValidationRequest;
use crate::chatsession::Text;

//This message should be sent to signify a client connecting to the server
#[derive(Message)]
pub struct Register {
    pub id: String,
    pub room_id: String,
    pub validation: String,
    pub addr: Addr<ChatSession>
}

//This is the catch all message type for anything that needs to be communicated
#[derive(Message)]
pub struct Message {
    pub id: String,
    pub room_id: String,
    pub message: String,
    //TODO: Add timestamp
}

//This message is sent by a client to disconnect from a room or from the server completely
#[derive(Message)]
pub struct Disconnect {
    //session id
    pub id: String,
    //True => disconnect from the server completely & False => disconnect from room
    pub full_disconnect: bool,
}

//Server's bookkeeping of each client's session state
struct Client {
    room_id: String,
    state: ClientState,
    client_addr: Addr<ChatSession>,
}

//Internal state of the chat server
pub struct ChatServer {
    //Rooms have their own set of connecting clients
    //Maps room ids to list of clients
    rooms: HashMap<String, HashSet<Addr<ChatSession>>>,
    //Match ids to Client Actors
    clients: HashMap<String, Client>
}

impl ChatServer {
    pub fn new() -> ChatServer {
        ChatServer { 
            rooms: HashMap::new(),
            clients: HashMap::new()
        }
    }

    //Broadcasts a message to every client in a room
    pub fn broadcast_message(&self, room_id: String, id: String, message: String) {
        match self.rooms.get(&room_id) {
            None => {},
            Some(client_list) => {
                for client in client_list.iter() {
                    client.do_send(Text {
                        id: id.clone(),
                        message: message.clone()
                    });
                }
            },
        }
    }

    pub fn get_user_count(&self, room_id: &String) -> usize {
        match self.rooms.get(room_id) {
            None => {
                return 0;
            },
            Some(client_list) => {
                return client_list.len();
            },
        }
    }

}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

//Client is attempting to join a room
impl Handler<Register> for ChatServer {
    type Result = ();

    fn handle(&mut self, registration: Register, ctx: &mut Self::Context) -> Self::Result {
        //Handle case where client switches to another room ... send a disconnect signal
        self.handle(Disconnect {
            id: registration.id.clone(),
            full_disconnect: false,
        }, ctx);


        match self.rooms.get_mut(&registration.room_id) {
            None => {
                //Room doesn't exist...make a new one
                //Register client and update the server's internal state
                let mut hashset: HashSet<Addr<ChatSession>> = HashSet::new();
                hashset.insert(registration.addr.clone());

                self.rooms.insert(registration.room_id.clone(), hashset);
                self.clients.insert(registration.id.clone(), Client {
                    room_id: registration.room_id.clone(),
                    state: ClientState::VALIDATED(registration.room_id.clone()),
                    client_addr: registration.addr.clone()
                });

                registration.addr.do_send(ClientState::VALIDATED(registration.room_id.clone()));
                println!("User {} has join room {}", registration.id, registration.room_id);
                self.broadcast_message(registration.room_id.clone(),
                                       "Server".to_string(),
                                       format!("User {} has join the room ... Number of connected users: {}", registration.id, self.get_user_count(&registration.room_id)));
            },
            Some(client_list) => {
                if client_list.is_empty() {
                    //Room exists but is empty ... validate and register the connecting client
                    client_list.insert(registration.addr.clone());
                    self.clients.insert(registration.id.clone(), Client {
                        room_id: registration.room_id.clone(),
                        state: ClientState::VALIDATED(registration.room_id.clone()),
                        client_addr: registration.addr.clone()
                    });

                    registration.addr.do_send(ClientState::VALIDATED(registration.room_id.clone()));
                    println!("User {} has join room {}", registration.id, registration.room_id);
                    self.broadcast_message(registration.room_id.clone(),
                                           "Server".to_string(),
                                           format!("User {} has join the room ... Number of connected users: {}", registration.id, self.get_user_count(&registration.room_id)));

                } else {
                    //Send a validation request to every client in the room
                    for client in client_list.iter() {
                        client.do_send(ValidationRequest {
                            room_id: registration.room_id.clone(),
                            id: registration.id.clone(),
                            validation: registration.validation.clone(),
                            accept: false,
                        });
                    }
                    //Register the client but do not add into room
                    self.clients.insert(registration.id.clone(), Client {
                        room_id: registration.room_id.clone(),
                        state: ClientState::AWAITING_VALIDATION,
                        client_addr: registration.addr.clone()
                    });
                    registration.addr.do_send(ClientState::AWAITING_VALIDATION);
                }
            },
        }
        println!("Number of clients: {}", self.clients.len());
    }
}

impl Handler<ValidationRequest> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: ValidationRequest, _ctx: &mut Self::Context) -> Self::Result {
        //Don't validate client with peers who don't accept
        if msg.accept {
            match self.clients.get_mut(&msg.id) {
                //Fail silently for bogus validation requests that attempt to validate a non-existent user
                None => {},
                Some(client) => {
                    match self.rooms.get_mut(&msg.room_id) {
                        None => {},
                        Some(client_list) => {
                            //All sanity checks passed ... validate the client
                            client.state = ClientState::VALIDATED(msg.room_id.clone());
                            client.client_addr.do_send(ClientState::VALIDATED(msg.room_id.clone()));
                            client_list.insert(client.client_addr.clone());
                            self.broadcast_message(msg.room_id.clone(),
                                                   "Server".to_string(),
                                                   format!("User {} has join the room ... Number of connected users: {}", msg.id, self.get_user_count(&msg.room_id)));

                        },
                    }
                },
            }
        }
    }
}

impl Handler<Message> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Message, _: &mut Self::Context) -> Self::Result {
        self.broadcast_message(msg.room_id, msg.id, msg.message);
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _ctx: &mut Self::Context) -> Self::Result {
        //Pull client out the current room
        match self.clients.get(&msg.id) {
            None => {/*Do nothing*/},
            Some(client) => {
                match self.rooms.get_mut(&client.room_id) {
                    None => {
                        println!("Impossible situation: client registered under nonexistent room.");
                    },
                    Some(peers) => {
                        //Deregister the client from the current room ... broadcast disconnect to room
                        peers.remove(&client.client_addr);
                        let peer_count = peers.len();
                        //Only broadcast the disconnect message if the disconnecting client was validated
                        if let ClientState::VALIDATED(_) = client.state {
                            self.broadcast_message(client.room_id.clone(),
                                                   "Server".to_string(),
                                                   format!("{} disconnected ... Number of connected users: {}", msg.id, peer_count));
                        }
                    },
                }
            },
        }
        //Completely deregister the client
        if msg.full_disconnect {
            self.clients.remove(&msg.id);
            println!("Number of clients: {}", self.clients.len());
        }
    }
}