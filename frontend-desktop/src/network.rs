use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use core_common::LinkEndpoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    None,
    Server,
    Client,
}

impl NetworkMode {
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Server,
            Self::Server => Self::Client,
            Self::Client => Self::None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Server => "Server",
            Self::Client => "Client",
        }
    }
}

pub struct NetworkSettings {
    pub mode: NetworkMode,
    pub host: String,
    pub port: u16,
    pub status: String,
    pub is_connected: bool,
    listener: Option<TcpListener>,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            mode: NetworkMode::None,
            host: "127.0.0.1".to_string(),
            port: 1337,
            status: "Disabled".to_string(),
            is_connected: false,
            listener: None,
        }
    }
}

impl NetworkSettings {
    pub fn toggle_mode(&mut self) {
        self.disconnect();
        self.mode = self.mode.next();
        self.status = match self.mode {
            NetworkMode::None => "Disabled".to_string(),
            NetworkMode::Server => "Server mode, press C to bind".to_string(),
            NetworkMode::Client => "Client mode, press C to connect".to_string(),
        };
    }

    pub fn cycle_host(&mut self) {
        self.host = if self.host == "127.0.0.1" {
            "localhost".to_string()
        } else if self.host == "localhost" {
            "0.0.0.0".to_string()
        } else {
            "127.0.0.1".to_string()
        };
        self.status = format!("Host set to {}", self.host);
    }

    pub fn change_port(&mut self, delta: i16) {
        let new_port = self.port as i32 + i32::from(delta);
        if let Some(port) = u16::try_from(new_port).ok() {
            self.port = port;
            self.status = format!("Port set to {}", self.port);
        }
    }

    pub fn disconnect(&mut self) {
        self.is_connected = false;
        self.listener = None;
        self.status = "Disconnected".to_string();
    }

    pub fn connect(&mut self) -> Result<Option<Box<dyn LinkEndpoint + Send>>, String> {
        self.is_connected = false;
        self.listener = None;

        match self.mode {
            NetworkMode::None => {
                self.status = "Link disabled".to_string();
                Ok(None)
            }
            NetworkMode::Server => {
                let listener = TcpListener::bind(("0.0.0.0", self.port))
                    .map_err(|e: std::io::Error| e.to_string())?;
                listener
                    .set_nonblocking(true)
                    .map_err(|e: std::io::Error| e.to_string())?;
                self.listener = Some(listener);
                self.status = format!("Listening on {}", self.port);
                Ok(None)
            }
            NetworkMode::Client => {
                let socket: SocketAddr = format!("{}:{}", self.host, self.port)
                    .parse()
                    .map_err(|e: std::net::AddrParseError| e.to_string())?;
                let stream = TcpStream::connect_timeout(&socket, Duration::from_secs(1))
                    .map_err(|e: std::io::Error| e.to_string())?;
                stream
                    .set_nodelay(true)
                    .map_err(|e: std::io::Error| e.to_string())?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .map_err(|e: std::io::Error| e.to_string())?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .map_err(|e: std::io::Error| e.to_string())?;
                self.status = format!("Connected to {}", socket);
                self.is_connected = true;
                Ok(Some(Box::new(TcpLink { stream })))
            }
        }
    }

    pub fn poll_server(&mut self) -> Result<Option<Box<dyn LinkEndpoint + Send>>, String> {
        if self.mode != NetworkMode::Server {
            return Ok(None);
        }

        if self.listener.is_none() {
            return Ok(None);
        }

        match self.listener.as_ref().unwrap().accept() {
            Ok((stream, peer)) => {
                stream
                    .set_nodelay(true)
                    .map_err(|e: std::io::Error| e.to_string())?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .map_err(|e: std::io::Error| e.to_string())?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .map_err(|e: std::io::Error| e.to_string())?;
                self.status = format!("Connected to {}", peer);
                self.is_connected = true;
                self.listener = None;
                Ok(Some(Box::new(TcpLink { stream })))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[derive(Debug)]
struct TcpLink {
    stream: TcpStream,
}

impl LinkEndpoint for TcpLink {
    fn transfer_byte(&mut self, byte: u8) -> io::Result<u8> {
        self.stream.write_all(&[byte])?;
        let mut response = [0u8; 1];
        self.stream.read_exact(&mut response)?;
        Ok(response[0])
    }
}
