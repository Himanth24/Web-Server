mod thread_pool;

use thread_pool::ThreadPool;

use std::{
    env, fmt::format, fs, io::{BufRead, BufReader, Read, Write}, net::{TcpListener, TcpStream}, sync::{Arc, Mutex}, thread
};

use flate2::{write::GzEncoder, Compression};

use ctrlc;

struct Request {
    method: String,
    path: String,
    version: String,
    headers: Vec<String>,
    body: Option<Vec<u8>>,
}

impl Request {
    fn new(reader: &mut BufReader<TcpStream>) -> Self {
        let mut request_line = String::new();
        let mut headers: Vec<String> = Vec::new();

        if let Ok(_) = reader.read_line(&mut request_line) {
            let mut header_line = String::new();
            while let Ok(_) = reader.read_line(&mut header_line) {
                if header_line == "\r\n"{
                    break;
                }
                headers.push(header_line.trim().to_string());
                header_line.clear();
            }
        }

        let request_line_data: Vec<&str> = request_line.split(' ').collect();
        let mut request = Request {
            method: request_line_data[0].to_string(),
            path: request_line_data[1].to_string(),
            version: request_line_data[2].to_string(),
            headers,
            body: None,
        };

        let content_length: usize = request.get_header("content-length").parse().unwrap_or(0);
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).unwrap();

        request.body = Some(body);
        request
    }

    fn get_header(self: &Self, _target_header: &str) -> String{
        let _lower_target_header = _target_header.to_lowercase();
        let target_header = _lower_target_header.trim();
        self.headers
            .iter()
            .find_map(|header| {
                if header.to_lowercase().starts_with(target_header) {
                    Some(
                        header
                            .to_lowercase()
                            .replace(format!("{}: ", target_header).as_str(), "")
                            .trim()
                            .to_string(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default()
            .to_string()
    }
}

fn respond_with_body(
    stream: &mut TcpStream,
    content_type: &str,
    body: &[u8],
    accept_encoding: &str,
) {
    let (encoded_body, encoding_header): (Vec<u8>, String) = if accept_encoding.contains("gzip") {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(body).unwrap();
        let compressed = encoder.finish().unwrap();
        (compressed, "Content-Encoding: gzip\r\n".to_string())
    } else {
        (body.to_vec(), "".to_string())
    };

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\n{}Content-Length: {}\r\n\r\n",
        content_type,
        encoding_header,
        encoded_body.len()
    );
    
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&encoded_body);
}

fn handle_client(stream: TcpStream) {
    println!("Accepted new connection from {}", stream.peer_addr().unwrap());
    let mut reader = BufReader::new(stream);
    let request = Request::new(&mut reader);
    let method = request.method.as_str();

    match method {
        "GET" => {
            if request.path == "/" {
                let file_path = "public/index.html";
                match fs::read(&file_path) {
                    Ok(contents) => {
                        let content_type = mime_guess::from_path(&file_path)
                            .first_or_octet_stream()
                            .essence_str()
                            .to_string();
                        let accept_encoding = request.get_header("accept-encoding");
                        let mut stream = reader.into_inner();
                        respond_with_body(
                            &mut stream,
                            &content_type,
                            &contents,
                            &accept_encoding,
                        );
                    }
                    Err(err) => {
                        let mut stream = reader.into_inner();
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
                    }
                }

                return;
            }

            if request.path.starts_with("/public/") {
                let file_name = request.path.replacen("/public/", "", 1);
                let file_path = format!("public/{}",file_name);
                match fs::read(&file_path) {
                    Ok(contents) => {
                        let content_type = mime_guess::from_path(&file_path)
                            .first_or_octet_stream()
                            .essence_str()
                            .to_string();
                        let accept_encoding = request.get_header("accept-encoding");
                        let mut stream = reader.into_inner();
                        respond_with_body(
                            &mut stream, 
                            &content_type, 
                            &contents, 
                            &accept_encoding
                        );
                    }
                    Err(err) => {
                        let mut stream = reader.into_inner();
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
                    }
                }

                return;
            }

            if request.path == "/echo" {
                let mut stream = reader.into_inner();
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n");
                return;
            }

            if request.path.starts_with("/echo/") {
                let echo_text = request.path.replacen("/echo/", "", 1);
                let accept_encoding = request.get_header("accept-encoding");
                let mut stream = reader.into_inner();

                respond_with_body(
                    &mut stream,
                    "text/plain",
                    echo_text.as_bytes(),
                    &accept_encoding,
                );
                return;
            }

            if request.path == "/user-agent" {
                let user_agent = request.get_header("user-agent");
                let accept_encoding = request.get_header("accept-encoding");
                let mut stream = reader.into_inner();

                respond_with_body(
                    &mut stream,
                    "text/plain",
                    user_agent.as_bytes(),
                    &accept_encoding,
                );
                return;
            }

            if request.path.starts_with("/files/") {
                let file_name = request.path.replacen("/files/", "", 1);
                let env_args: Vec<String> = env::args().collect();
                let mut file_path_dir = env_args[2].clone();
                file_path_dir.push_str(&file_name);

                match fs::read(file_path_dir) {
                    Ok(contents) => {
                        let accept_encoding = request.get_header("accept-encoding");
                        let mut stream = reader.into_inner();
                        respond_with_body(
                            &mut stream,
                            "application/octet-stream",
                            &contents,
                            &accept_encoding,
                        );
                    }
                    Err(_) => {
                        let mut stream = reader.into_inner();
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
                    }
                }
                return;
            }

            // Default 404
            let mut stream = reader.into_inner();
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
        }

        "POST" => {
            if request.path.starts_with("/files/") {
                let file_name = request.path.replacen("/files/", "", 1);
                let env_args: Vec<String> = env::args().collect();
                let mut file_path_dir = env_args[2].clone();
                file_path_dir.push_str(&file_name);
                let request_body = request.body.unwrap();

                let _ = fs::write(file_path_dir, request_body);
                let mut stream = reader.into_inner();
                let _ = stream.write_all(b"HTTP/1.1 201 Created\r\n\r\n");
                return;
            }

            let mut stream = reader.into_inner();
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
        }

        _ => {
            let mut stream = reader.into_inner();
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
        }
    }
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();
    let pool = Arc::new(Mutex::new(ThreadPool::new(4)));

    let pool_for_shutdown = Arc::clone(&pool);

    ctrlc::set_handler(move || {
        let mut pool = pool_for_shutdown.lock().unwrap();
        pool.shutdown();
        std::process::exit(0);
    }).expect("Failed to set CTRL+C handler");

    if let Err(err) = std::panic::catch_unwind(|| {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let pool = Arc::clone(&pool);
                    pool.lock().unwrap().execute(|| {
                        handle_client(stream);
                    });
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }
    }){
        eprintln!("Main thread panicked: {:?}", err);
        pool.lock().unwrap().shutdown();
    }
}