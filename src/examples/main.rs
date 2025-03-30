use std::{io::{Error, ErrorKind, Read, Write}, net::{TcpListener, TcpStream}, rc::Rc, sync::{mpsc, Arc, Mutex}, thread::{self, sleep, JoinHandle}, time::Duration};

use sha1::{Sha1, Digest};
use std::sync::mpsc::{Receiver, Sender};
use event_emitter_rs::EventEmitter;
use base64::{engine::general_purpose::STANDARD, Engine};


#[derive(Clone)]
struct Server { 
    opts: ServerOpts,
    conn: Arc<TcpListener>,
    receiver: Arc<Receiver<WSConnection>>
}
trait Message { 
    fn serialize(&self) -> String;
    fn deerialize(str: String) -> Self where Self: Sized;
}


struct WSConnection {  
    stream: TcpStream,
    emitter : EventEmitter
}

impl WSConnection { 
    fn send<T: Message + Send + 'static> (&mut self, value: T) { 
        self.emitter.emit("",value.serialize());
    }

    fn receive<T: Message + Send + 'static, F: Fn(T) + Send + 'static + Sync >(&mut self, func: F) { 
        self.emitter.on("onmessage", move |value| func(T::deerialize(value)));
    }
}

#[derive(Clone)]
struct ServerOpts { 
    host: String,
    port: usize
}

impl Server { 
    pub fn new(host: String, port: usize) -> Result<(Self,Sender<WSConnection>), Error> { 
        match TcpListener::bind(format!("{host}:{port}")) { 
            Ok(listener) => {
                let (sender, receiver) = mpsc::channel();
                return Ok((Self { 
                    opts: ServerOpts { host, port},
                    conn: Arc::new(listener),
                    receiver: Arc::new(receiver)
                }, sender));
            }, Err(err) => { 
                return Err(err);
            }
        }
        
    }

    pub fn accept(server: Server, sender: Sender<WSConnection>) -> JoinHandle<()> { 
        thread::spawn(move || { 
            println!("listening");
            while let Ok((mut conn, address)) = server.conn.accept() { 
                println!("connection is here");
                let mut buf = vec![0u8; 1048];
                loop { 
                    match conn.read(&mut buf) { 
                        Ok(0) => { 
                            break;
                        }
                        Ok(n) => { 
                            println!("read {n} bytes");
                            let data_str = String::from_utf8_lossy(&buf[..n]).to_string();
                            if let Some(key) = parse_key(data_str) { 
                                let accept_key = generate_key(key);
                                let response = format!(
                                    "HTTP/1.1 101 Switching Protocols\r\n\
                                     Upgrade: websocket\r\n\
                                     Connection: Upgrade\r\n\
                                     Sec-WebSocket-Accept: {}\r\n\
                                     \r\n",
                                    accept_key
                                );
                                conn.write_all(response.as_bytes()).unwrap();
                                conn.flush().unwrap();
                                let  mut conn_clone = conn.try_clone().unwrap();
                                let ws_conn = WSConnection { stream : conn_clone, emitter: EventEmitter::new() };
                                sender.send(ws_conn);
                                let handle  = thread::spawn(move || handle_websocket(&mut conn));
                                let _ = handle.join().unwrap();
                            } 
                        },
                        Err(err) => { 
                            if(err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut) { 
                                println!("error  {err:?}");
                                break;
                            } else { 
                                break;
                            } 
                        }
                    
                    }
                    break;
                }
                break;
            } 
        })
    }
}

fn main() { 
    let (server, sender) = Server::new("0.0.0.0".to_string(), 8080).unwrap();
    let handle = Server::accept(server.clone(), sender);
    sleep(Duration::from_secs(2));
    println!("control reaches here");
    let conn = server.receiver.recv().unwrap();
    println!("conn "); 
    let _ = handle.join().unwrap();
    
}

// fn main() { 
//     let listener = Server::new("0.0.0.0".to_string(), 8080).unwrap().conn;
//     println!("listening");
//     while let Ok((mut conn, address)) = listener.accept() { 
//         println!("connection is here");
//         let mut buf = vec![0u8; 1048];
//         let mut total_bytes_read = 0 as usize;
//         //let _ = conn.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
//         loop { 
//             match conn.read(&mut buf) { 
//                 Ok(0) => { 
//                     break;
//                 }
//                 Ok(n) => { 
//                     println!("read {n} bytes");
//                     let data_str = String::from_utf8_lossy(&buf[..n]).to_string();
//                     if let Some(key) = parse_key(data_str) { 
//                         let accept_key = generate_key(key);
//                         let response = format!(
//                             "HTTP/1.1 101 Switching Protocols\r\n\
//                              Upgrade: websocket\r\n\
//                              Connection: Upgrade\r\n\
//                              Sec-WebSocket-Accept: {}\r\n\
//                              \r\n",
//                             accept_key
//                         );
//                         conn.write_all(response.as_bytes()).unwrap();
//                         conn.flush().unwrap();
//                         let  mut conn_clone = conn.try_clone().unwrap();
//                         let handle = thread::spawn(move ||{  handle_websocket(&mut conn_clone);});
//                         let _ = handle.join().unwrap(); 
//                     } 
//                 },
//                 Err(err) => { 
//                     if(err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut) { 
//                         println!("error  {err:?}");
//                         break;
//                     } else { 
//                         break;
//                     } 
//                 }
//             }
//         }
        
//     }
    
// }

fn handle_websocket(conn: &mut TcpStream) { 
    println!("connection : {conn:?}");
    let mut buf = vec![0u8; 1024];
    loop { 
        match conn.read(&mut buf) {
            Ok(n)  => { 
                if n > 0 { 
                    println!("received ws frames : {:?}", &buf[..n]);
                    if let Some(message) = parse_websocket_frame(&buf[..n]) {
                        println!("Decoded WebSocket message: {}", message);
                        send_websocket_message(conn, &message);
                    }
                } else { 
                    println!("client disconnected");
                    break;
                }
            }
            Err(_) => todo!(),
        }
    }
}

fn parse_key(data: String) -> Option<String>{ 
    for line in data.lines() { 
        if line.contains("Sec-WebSocket-Key") { 
            if let Some(key) = line.strip_prefix("Sec-WebSocket-Key: ") { 
                return Some(key.to_string())
            }
        }
    }
    None
}

fn generate_key(key: String) -> String{ 
    let magic_string = format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let mut hasher = Sha1::new();
    hasher.update(magic_string.as_bytes());
    let result = hasher.finalize();
    STANDARD.encode(result)

}

fn send_websocket_message(conn: &mut TcpStream, message: &str) {
    let mut frame = vec![0x81]; // FIN + Text frame (Opcode 0x1)

    let payload_len = message.len();
    if payload_len <= 125 {
        frame.push(payload_len as u8);
    } else if payload_len <= 65535 {
        frame.push(126);
        frame.extend_from_slice(&(payload_len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(payload_len as u64).to_be_bytes());
    }

    frame.extend_from_slice(message.as_bytes());
    conn.write_all(&frame).unwrap();
    conn.flush().unwrap();
}

fn parse_websocket_frame(data: &[u8]) -> Option<String> {
    if data.len() < 2 {
        return None; // Too short to be a valid frame
    }

    let fin = (data[0] & 0b1000_0000) != 0;
    let opcode = data[0] & 0b0000_1111;
    let masked = (data[1] & 0b1000_0000) != 0;
    let mut payload_len = (data[1] & 0b0111_1111) as usize;

    let mut index = 2;

    if payload_len == 126 {
        if data.len() < 4 {
            return None;
        }
        payload_len = u16::from_be_bytes([data[2], data[3]]) as usize;
        index += 2;
    } else if payload_len == 127 {
        if data.len() < 10 {
            return None;
        }
        payload_len = u64::from_be_bytes([
            data[2], data[3], data[4], data[5], data[6], data[7], data[8], data[9],
        ]) as usize;
        index += 8;
    }

    let mask_key = if masked {
        if data.len() < index + 4 {
            return None;
        }
        Some([data[index], data[index + 1], data[index + 2], data[index + 3]])
    } else {
        None
    };

    index += if masked { 4 } else { 0 };

    if data.len() < index + payload_len {
        return None;
    }

    let mut payload = data[index..index + payload_len].to_vec();

    if let Some(mask_key) = mask_key {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask_key[i % 4];
        }
    }

    match opcode {
        0x1 => String::from_utf8(payload).ok(), // Text frame
        0x8 => {
            println!("Received Close Frame.");
            None // Handle closing the connection properly
        }
        0x9 => {
            println!("Received Ping Frame, responding with Pong.");
            // Implement Ping-Pong response if needed
            None
        }
        _ => None,
    }
}

