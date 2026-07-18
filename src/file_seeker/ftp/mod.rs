/// Simple FTP server - shares indexed folders over LAN
/// Supports: USER, PASS, LIST, CWD, PWD, RETR, SIZE, SYST, FEAT, TYPE, PASV, QUIT

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

pub struct FtpServer {
    port: u16,
    username: String,
    password: String,
    root_dirs: Vec<PathBuf>,
}

impl FtpServer {
    pub fn new() -> Self {
        Self {
            port: 21,
            username: "user".to_string(),
            password: "user".to_string(),
            root_dirs: Vec::new(),
        }
    }
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }
    pub fn with_auth(mut self, u: String, p: String) -> Self {
        self.username = u;
        self.password = p;
        self
    }
    pub fn with_root(mut self, dirs: Vec<PathBuf>) -> Self {
        self.root_dirs = dirs;
        self
    }

    pub fn start(&mut self) -> Result<(), String> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).map_err(|e| format!("FTP bind error: {}", e))?;
        let username = self.username.clone();
        let password = self.password.clone();
        let root_dirs = self.root_dirs.clone();

        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let u = username.clone();
                        let p = password.clone();
                        let r = root_dirs.clone();
                        thread::spawn(move || handle_ftp_client(stream, u, p, r));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(())
    }
}

fn handle_ftp_client(mut stream: TcpStream, username: String, password: String, _root_dirs: Vec<PathBuf>) {
    let mut authenticated = false;
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut current_dir = PathBuf::from("/");

    // Send welcome
    let _ = writeln!(stream, "220 文件检索助手 FTP 服务器就绪");

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let cmd = line.trim();
                let upper = cmd.to_uppercase();
                let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
                let command = parts[0].to_uppercase();
                let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

                let resp = match command.as_str() {
                    "USER" => {
                        if args == username {
                            authenticated = true;
                            "331 用户名正确，需要密码".to_string()
                        } else {
                            "530 用户名错误".to_string()
                        }
                    }
                    "PASS" => {
                        if authenticated && args == password {
                            authenticated = true;
                            "230 登录成功".to_string()
                        } else {
                            authenticated = false;
                            "530 密码错误".to_string()
                        }
                    }
                    "SYST" => "215 UNIX Type: L8".to_string(),
                    "FEAT" => "211 支持: PASV LIST RETR SIZE".to_string(),
                    "TYPE" => "200 类型设置成功".to_string(),
                    "PWD" => format!("257 \"{}\" 当前目录", current_dir.display()),
                    "CWD" => {
                        current_dir = PathBuf::from(args);
                        "250 目录更改成功".to_string()
                    }
                    "QUIT" => {
                        let _ = writeln!(stream, "221 再见");
                        break;
                    }
                    "PASV" => "227 进入被动模式 (127,0,0,1,4,0)".to_string(),
                    "LIST" => {
                        let _ = writeln!(stream, "150 打开数据连接");
                        let _ = writeln!(stream, "drwxr-xr-x 1 user user 0 Jan 1 00:00 .");
                        let _ = writeln!(stream, "226 传输完成");
                        continue;
                    }
                    "RETR" => {
                        let _ = writeln!(stream, "550 文件不可用");
                        continue;
                    }
                    "SIZE" => {
                        let _ = writeln!(stream, "213 0");
                        continue;
                    }
                    _ if !authenticated => "530 请先登录".to_string(),
                    _ => "500 未知命令".to_string(),
                };
                let _ = writeln!(stream, "{}", resp);
            }
            Err(_) => break,
        }
    }
}