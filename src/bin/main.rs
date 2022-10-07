
use tokio::net::{TcpListener, TcpStream};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
//use bytes::{BytesMut,BufMut};
use std::sync::{Arc,Mutex};
//use dashmap::DashMap;
use tokio::sync::broadcast;
use tokio::fs::File;

const IP : &str = "0.0.0.0";
const PORT : &str = "8080";

#[derive(Clone,Debug)]
struct Message{
    id : u32,
    file_size : u32,
    text : Vec<u8>
}

impl Message{
    
    fn new(id : u32, file_size : u32, text : Vec<u8>) -> Self{
        Message{
            id : id,
            file_size : file_size,
            text : text
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //initialize database
    let db : Arc<Mutex<Vec<Message>>> = Arc::new(Mutex::new(Vec::new()));
    let (tx, _) = broadcast::channel::<(Message,SocketAddr)>(16);
    let listener = TcpListener::bind(format!("{}:{}",IP,PORT)).await?;
    loop {
        let db = db.clone();
        let (socket,addr) = listener.accept().await?;
        let tx = tx.clone();
        tokio::spawn(async move {
            handler(socket,addr,tx,db).await;
        });
    }
}

async fn handler(mut socket : TcpStream, addr : SocketAddr, tx : broadcast::Sender<(Message,SocketAddr)>, db : Arc<Mutex<Vec<Message>>>){
    let mut api_version = [0u8;4];
    socket.read_exact(&mut api_version).await.unwrap();
    let mut rx = tx.subscribe();
    loop{
        let mut message_size = [0u8;4];
        tokio::select!(
            _result = socket.read_exact(&mut message_size) => {
                let message_size = u32::from_le_bytes(message_size);
                let mut buf = vec![0;message_size as usize];
                socket.read_exact(&mut buf).await.unwrap();
                let function : u16 = u16::from_le_bytes(buf[0..2].try_into().unwrap());
                match function {
                    0 => {
                        let file_size = u32::from_le_bytes(buf[2..6].try_into().unwrap());
                        let text_size = message_size-file_size-6;
                        let text = match text_size{
                            0 => {
                                Vec::<u8>::new()
                            },
                            _ => {
                                buf[6..(text_size as usize + 6)].to_vec()
                            }
                        };
                        if file_size!=0 || text_size!=0{
                            let mut msg = Message::new(0,file_size,text);
                            {
                                let mut db = db.lock().unwrap();
                                msg.id = db.len().try_into().unwrap();
                                let msg2 = msg.clone();
                                db.push(msg2);
                            }
                            if file_size!=0{
                                let mut file = File::create(format!("{}",msg.id)).await.unwrap();
                                file.write_all(&buf[(text_size as usize+6)..(message_size as usize)]).await.unwrap();
                            }
                            tx.send((msg, addr)).unwrap();
                        }
                    },
                    1 => {
                        // not tested
                        let mut start = usize::from_le_bytes(buf[2..6].try_into().unwrap());
                        let mut end = usize::from_le_bytes(buf[6..10].try_into().unwrap());
                        let mut msgs : Vec<Message> = Vec::new();
                        {
                            let db = db.lock().unwrap();
                            let last_msg = db.len();
                            if start<=end && end<last_msg+2{
                                if start==end{
                                    start = last_msg-start;
                                    end = last_msg+1;
                                }
                                for msg in &db[start..end]{
                                    msgs.push(msg.clone());
                                }
                            }       
                        }
                        for msg in msgs{
                            let msg_size = 10+msg.text.len();
                            let msg_size = msg_size.to_le_bytes();
                            let t = [0u8,1u8];
                            let id = msg.id.to_le_bytes();
                            let file_size = msg.file_size.to_le_bytes();
                            socket.write_all(&msg_size).await.unwrap();
                            socket.write_all(&t).await.unwrap();
                            socket.write_all(&id).await.unwrap();
                            socket.write_all(&file_size).await.unwrap();
                            socket.write_all(&msg.text).await.unwrap();
                        }
                    },
                    2 => {
                        // get file 
                        // not tested
                        let file_name = u32::from_le_bytes(buf[2..6].try_into().unwrap());
                        let mut file = File::open(format!("{}",file_name)).await.unwrap();
                        let mut data = Vec::new();
                        file.read_buf(&mut data).await.unwrap();
                        let msg_size = ((data.len()+2) as u32).to_le_bytes();
                        let op_code = [0u8,2u8];
                        socket.write_all(&msg_size).await.unwrap();
                        socket.write_all(&op_code).await.unwrap();
                        socket.write_all(&buf[2..6]).await.unwrap();
                        socket.write_all(&data).await.unwrap();
                    }
                    _ =>{
                        // unimplemented
                    }
                }
            }
            result = rx.recv() => {
                let (msg, other_addr) = result.unwrap();
                if addr!=other_addr{
                    let msg_size : u32 = 10+msg.text.len() as u32;
                    let msg_size_byte = msg_size.to_le_bytes();
                    let t = [0u8,1u8];
                    let id = msg.id.to_le_bytes();
                    let file_size = msg.file_size.to_le_bytes();
                    socket.write_all(&msg_size_byte).await.unwrap();
                    socket.write_all(&t).await.unwrap();
                    socket.write_all(&id).await.unwrap();
                    socket.write_all(&file_size).await.unwrap();
                    socket.write_all(&msg.text).await.unwrap();
                }
            }
        );
    }
}
