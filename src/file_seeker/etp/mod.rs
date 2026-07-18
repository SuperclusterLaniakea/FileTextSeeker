/// ETP (Everything Transfer Protocol) / FTP server
///
/// ETP is a custom protocol used by Everything for client-server search.
/// This implements a basic ETP server that allows remote search.

use std::sync::Arc;
use std::io::{BufRead, Write, BufReader};
use std::net::{TcpListener, TcpStream};
use std::thread;
use crate::file_seeker::engine::Engine;
use crate::file_seeker::types::SearchOptions;

/// ETP/FTP server
pub struct EtpServer {
    engine: Arc<Engine>,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    running: bool,
    welcome_message: Option<String>,
}

impl EtpServer {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            port: 21,
            username: None,
            password: None,
            running: false,
            welcome_message: Some("Welcome to 文件检索助手ETP Server".to_string()),
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_auth(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }

    /// Start the ETP server
    pub fn start(&mut self) -> Result<(), String> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr)
            .map_err(|e| format!("Failed to start ETP server: {}", e))?;

        self.running = true;
        println!("ETP server started on {}", addr);

        for stream in listener.incoming() {
            if !self.running {
                break;
            }

            match stream {
                Ok(stream) => {
                    let engine = self.engine.clone();
                    let username = self.username.clone();
                    let password = self.password.clone();
                    let welcome = self.welcome_message.clone();
                    thread::spawn(move || {
                        handle_client(stream, engine, username, password, welcome);
                    });
                }
                Err(e) => {
                    eprintln!("ETP connection error: {}", e);
                }
            }
        }

        Ok(())
    }

    pub fn stop(&mut self) {
        self.running = false;
    }
}

/// Handle a single ETP client connection
fn handle_client(
    mut stream: TcpStream,
    engine: Arc<Engine>,
    _username: Option<String>,
    _password: Option<String>,
    welcome: Option<String>,
) {
    // Send welcome message
    let welcome_msg = welcome.unwrap_or_else(|| "220 文件检索助手ETP Server ready".to_string());
    let _ = writeln!(stream, "{}", welcome_msg);

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut authenticated = false;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let cmd = line.trim();
                let response = process_etp_command(cmd, &engine, &mut authenticated);
                if let Err(e) = writeln!(stream, "{}", response) {
                    eprintln!("Failed to send response: {}", e);
                    break;
                }
                if cmd.to_uppercase().starts_with("QUIT") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// Process an ETP command
fn process_etp_command(cmd: &str, engine: &Arc<Engine>, authenticated: &mut bool) -> String {
    let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
    let command = parts[0].to_uppercase();
    let args = parts.get(1).copied().unwrap_or("");

    match command.as_str() {
        "USER" => {
            *authenticated = false;
            "331 User name okay, need password".to_string()
        }
        "PASS" => {
            if args.is_empty() {
                *authenticated = true;
                "230 User logged in, proceed".to_string()
            } else {
                *authenticated = true;
                "230 User logged in, proceed".to_string()
            }
        }
        "QUIT" => {
            "221 Goodbye".to_string()
        }
        "SEARCH" if *authenticated => {
            let options = SearchOptions::default();
            let results = engine.search(args, &options).unwrap_or_default();
            format!("213 {} results", results.len())
        }
        "NOOP" => "200 OK".to_string(),
        "HELP" => {
            "214 Supported commands: USER, PASS, SEARCH, QUIT, NOOP, HELP".to_string()
        }
        _ if !*authenticated => {
            "530 Please login with USER and PASS".to_string()
        }
        _ => {
            "500 Unknown command".to_string()
        }
    }
}

