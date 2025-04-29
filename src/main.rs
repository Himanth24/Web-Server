mod thread_pool;

use thread_pool::ThreadPool;

use std::{
    env, fs, io::{self, ErrorKind, BufRead, BufReader, Read, Write}, 
    net::{self, TcpListener, TcpStream}, 
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, 
    thread, time::Duration
};

use flate2::{write::GzEncoder, Compression};

struct Request {
    method: String,
    path: String,
    version: String,
    headers: Vec<String>,
    body: Option<Vec<u8>>,
}

impl Request {
    fn new(reader: &mut BufReader<&mut TcpStream>) -> Result<Self, std::io::Error> {
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

        println!("{}", request_line_data.len());
        if request_line_data.len() != 3 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Malformed request line",
            ));
        }

        let mut request = Request {
            method: request_line_data.get(0).unwrap_or(&"").to_string(),
            path: request_line_data.get(1).unwrap_or(&"").to_string(),
            version: request_line_data.get(2).unwrap_or(&"").to_string(),
            headers,
            body: None,
        };

        let content_length: usize = request.get_header("content-length").parse().unwrap_or(0);
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).unwrap();

        request.body = Some(body);
        Ok(request)
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
    
    if let Err(e) = stream.write_all(response.as_bytes()) {
        eprintln!("Error writing response header: {}", e);
        return;
    }
    
    if let Err(e) = stream.write_all(&encoded_body) {
        eprintln!("Error writing response body: {}", e);
        return;
    }
    
    // Ensure the data is sent immediately
    if let Err(e) = stream.flush() {
        eprintln!("Error flushing stream: {}", e);
    }
    
    // Shutdown the write side of the connection
    if let Err(e) = stream.shutdown(std::net::Shutdown::Write) {
        eprintln!("Error shutting down write side: {}", e);
    }
}

fn handle_client(mut stream: TcpStream) {
    // Set socket options for quick cleanup
    if let Err(e) = stream.set_nodelay(true) {
        eprintln!("Failed to set TCP_NODELAY: {}", e);
    }
    
    if let Err(e) = stream.set_read_timeout(Some(std::time::Duration::from_secs(5))) {
        eprintln!("Failed to set read timeout: {}", e);
    }
    
    if let Err(e) = stream.set_write_timeout(Some(std::time::Duration::from_secs(30))) {
        eprintln!("Failed to set write timeout: {}", e);
    }
    
    match stream.peer_addr() {
        Ok(addr) => println!("Accepted new connection from {}", addr),
        Err(e) => eprintln!("Failed to get peer address: {}", e),
    }
    
    loop {
        let mut reader = BufReader::new(&mut stream);
        let request = match Request::new(&mut reader) {
            Ok(req) => req,
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                eprintln!("Connection timed out or was idle for too long");
                return ;
            }
            Err(e) => {
                println!("Error reading request {}",e.kind());
                return ;
            }
        };
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
                        Err(e) => {
                            eprintln!("Error reading file {}: {}", file_path, e);
                            let mut stream = reader.into_inner();
                            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
                        }
                    }

                    return;
                }

                if request.path.starts_with("/public/") {
                    let file_name = request.path.replacen("/public/", "", 1);
                    let file_path = format!("public/{}", file_name);
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
                        Err(e) => {
                            eprintln!("Error reading file {}: {}", file_path, e);
                            let mut stream = reader.into_inner();
                            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
                        }
                    }

                    return;
                }

                if request.path == "/echo" {
                    let mut stream = reader.into_inner();
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                    let _ = stream.flush();
                    let _ = stream.shutdown(net::Shutdown::Both);
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
                    if env_args.len() < 3 {
                        let mut stream = reader.into_inner();
                        let _ = stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\n\r\n");
                        return;
                    }
                    
                    let mut file_path_dir = env_args[2].clone();
                    file_path_dir.push_str(&file_name);

                    match fs::read(&file_path_dir) {
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
                        Err(e) => {
                            eprintln!("Error reading file {}: {}", file_path_dir, e);
                            let mut stream = reader.into_inner();
                            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
                        }
                    }
                    return;
                }

                // Default 404
                let mut stream = reader.into_inner();
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
                let _ = stream.flush();
                let _ = stream.shutdown(net::Shutdown::Both);
            }

            "POST" => {
                if request.path.starts_with("/files/") {
                    let file_name = request.path.replacen("/files/", "", 1);
                    let env_args: Vec<String> = env::args().collect();
                    if env_args.len() < 3 {
                        let mut stream = reader.into_inner();
                        let _ = stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\n\r\n");
                        return;
                    }
                    
                    let mut file_path_dir = env_args[2].clone();
                    file_path_dir.push_str(&file_name);
                    
                    if let Some(request_body) = request.body {
                        match fs::write(&file_path_dir, request_body) {
                            Ok(_) => {
                                let mut stream = reader.into_inner();
                                let _ = stream.write_all(b"HTTP/1.1 201 Created\r\n\r\n");
                            },
                            Err(e) => {
                                eprintln!("Error writing file {}: {}", file_path_dir, e);
                                let mut stream = reader.into_inner();
                                let _ = stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\n\r\n");
                            }
                        }
                    } else {
                        let mut stream = reader.into_inner();
                        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
                    }
                    return;
                }

                let mut stream = reader.into_inner();
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
                let _ = stream.flush();
                let _ = stream.shutdown(net::Shutdown::Both);
            }

            _ => {
                let mut stream = reader.into_inner();
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
            }
        }
        let connection_type = request.get_header("connection");
        let version = request.version.trim();
        let should_close = match version {
            "HTTP/1.0" => connection_type != "keep-alive",
            "HTTP/1.1" => connection_type == "close",
            &_ => true
        };

        if should_close { 
                break;
        }
    }
}

fn main() {
    // Set up a flag to track shutdown state
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    
    // Set up socket cleanup monitor
    let active_connections = Arc::new(Mutex::new(Vec::<std::net::SocketAddr>::new()));
    let active_clone = Arc::clone(&active_connections);
    
    // Set up ctrl+c handler
    ctrlc::set_handler(move || {
        println!("Received SIGINT (Ctrl+C), shutting down...");
        r.store(false, Ordering::SeqCst);
        
        // Log active connections during shutdown
        if let Ok(conns) = active_clone.lock() {
            if !conns.is_empty() {
                println!("Active connections during shutdown: {:?}", conns);
            }
        }
    }).expect("Failed to set CTRL+C handler");

    // Create the thread pool with an optimized number of workers
    let num_cpus = std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4);
    let pool = Arc::new(Mutex::new(ThreadPool::new(num_cpus)));
    println!("Created thread pool with {} workers", num_cpus);
    
    // Try to bind to the address
    let listener = match TcpListener::bind("127.0.0.1:4221") {
        Ok(listener) => {
            println!("Server listening on http://127.0.0.1:4221");
            listener
        },
        Err(e) => {
            eprintln!("Failed to bind to address: {}", e);
            return;
        }
    };
    
    // Set socket options on the listener
    if let Err(e) = listener.set_nonblocking(true) {
        eprintln!("Failed to set non-blocking mode: {}", e);
        return;
    }
    
    // Main server loop
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, addr)) => {
                // Track the connection
                if let Ok(mut conns) = active_connections.lock() {
                    conns.push(addr);
                }
                
                let conn_track = Arc::clone(&active_connections);
                let client_addr = addr;
                
                let pool_clone = Arc::clone(&pool);
                if let Ok(mut pool) = pool_clone.lock() {
                    pool.execute(move || {
                        // Process the client request
                        handle_client(stream);
                        
                        // Remove from active connections when done
                        if let Ok(mut conns) = conn_track.lock() {
                            if let Some(pos) = conns.iter().position(|x| *x == client_addr) {
                                conns.swap_remove(pos);
                            }
                        }
                    });
                } else {
                    eprintln!("Failed to acquire lock on thread pool");
                    
                    // Remove the connection we couldn't handle
                    if let Ok(mut conns) = active_connections.lock() {
                        if let Some(pos) = conns.iter().position(|x| *x == addr) {
                            conns.swap_remove(pos);
                        }
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connection available, sleep for a bit
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            Err(e) => {
                eprintln!("Error accepting connection: {}", e);
                break;
            }
        }
    }
    
    // Shutdown the thread pool
    println!("Shutting down server...");
    if let Ok(mut pool) = pool.lock() {
        pool.shutdown();
    } else {
        eprintln!("Failed to acquire lock on thread pool for shutdown");
    }
    
    println!("Server shutdown complete");
}