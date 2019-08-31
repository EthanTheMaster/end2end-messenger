extern crate actix_web;
extern crate serde;
extern crate serde_json;
extern crate openssl;
extern crate actix;
extern crate hex;
extern crate rand;
extern crate base64;

mod chatserver;
mod chatsession;
mod test;

use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix::prelude::*;
use actix_web_actors::ws;

use std::fs::File;
use std::io::Read;

use rand::prelude::*;

use serde::Deserialize;

use crate::chatserver::ChatServer;
use crate::chatsession::{ChatSession, ClientState};
use actix_web::web::Path;
use std::time::Instant;
use openssl::ssl::{SslAcceptor, SslMethod, SslFiletype};

//Converts contents of a file into a HttpResponse
fn generate_static_asset_response(path: String) -> HttpResponse {
    let file = File::open(format!("static/{}", path));
    match file {
        Ok(mut f) => {
            let mut buf: Vec<u8> = Vec::new();
            let read = f.read_to_end(&mut buf);
            match read {
                Ok(_) => {
                    return HttpResponse::Ok().body(buf);
                },
                Err(_) => {
                    return HttpResponse::InternalServerError().body("Something went wrong...");
                },
            }
        },
        Err(_) => {
            return HttpResponse::NotFound().body("404");
        },
    }
}

fn index(_req: HttpRequest) -> HttpResponse {
    generate_static_asset_response(String::from("index.html"))
}

#[derive(Deserialize)]
struct AssetPath(String);
fn get_asset(req: Path<AssetPath>) -> HttpResponse {
    generate_static_asset_response(req.0.clone())
}

//Set up client with a session ... called every time a WebSocket client hits WebSocket endpoint
fn chat(req: HttpRequest, stream: web::Payload, server: web::Data<Addr<ChatServer>>) -> Result<HttpResponse, Error> {
    //Set up the session
    let mut rng = rand::thread_rng();
    let id_bytes: [u8; 8] = rng.gen();

    let session = ChatSession {
        id: hex::encode(id_bytes),
        server_addr: server.get_ref().clone(),
        state: ClientState::INIT,
        last_heartbeat: Instant::now(),
    };

    println!("connected user {}!", session.id);
    let resp = ws::start(session, &req, stream);
    resp
}

#[derive(Deserialize)]
struct ServerConfig {
    ip: String,
    private_key_path: String,
    certificate_path: String,
}

fn main() {
    let sys = System::new("chatserver");
    let chat_server = chatserver::ChatServer::new().start();

    //Configurations
    let config_file = std::fs::read("./config.json").unwrap();
    let server_config: ServerConfig = serde_json::from_str(&String::from_utf8(config_file).unwrap()).unwrap();

    //Place SSL certs in the project's source directory
    //Self-sign certificate: `openssl req -x509 -newkey rsa:4096 -nodes -keyout key.pem -out cert.pem -days 365 -subj '/CN=ENDPOINT_IP'`

    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    builder
        .set_private_key_file(server_config.private_key_path, SslFiletype::PEM)
        .unwrap();
    builder.set_certificate_chain_file(server_config.certificate_path).unwrap();

    println!("Server is running at: https://{}", server_config.ip);
    HttpServer::new(move || {
        App::new()
            //The chat server address should be shared with every connecting client
            .data(chat_server.clone())
            .service(web::resource("/").to(index))
            .route("/chat", web::get().to(chat))
            .route("/{path}", web::get().to(get_asset))
    })
        .bind_ssl(server_config.ip, builder)
        .unwrap()
        .run()
        .unwrap();

    let _ = sys.run();
}
