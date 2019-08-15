# What is this?
This is a novel end-to-end messaging service. Note that there is no guarantee that this service is resilient to any attacks of any kind. Use this service with caution if security is of the utmost importance. 
# Background
The backend is written in Rust and is handled by the `actix-web` library. Communication is done with WebSocket, and encryption/decryption is done completely within the client's web browser. The frontend uses the `CryptoJS` library to handle the AES-256 CBC encryption/decryption. 
