use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{info, error, debug, warn};
use crate::session::Session;

pub struct ProxyServer {
    listen_addr: SocketAddr,
    upstream_addr: SocketAddr,
    socket: Arc<UdpSocket>,
    sessions: Arc<RwLock<std::collections::HashMap<SocketAddr, Arc<Session>>>>,
    log_dir: std::path::PathBuf,
}

impl ProxyServer {
    pub fn new(listen_addr: SocketAddr, upstream_addr: SocketAddr) -> Result<Self> {
        // Create logs directory
        let log_dir = std::path::PathBuf::from("logs");
        std::fs::create_dir_all(&log_dir)
            .context("Failed to create logs directory")?;

        // Bind UDP socket for listening to clients
        // We'll use this same socket for forwarding to upstream as well
        let socket = std::net::UdpSocket::bind(listen_addr)
            .context("Failed to bind to listen address")?;
        
        socket.set_nonblocking(true)
            .context("Failed to set socket to non-blocking")?;

        let socket = UdpSocket::from_std(socket.into())?;

        info!(
            "Proxy configured: listening on {}, forwarding to {}",
            listen_addr, upstream_addr
        );

        Ok(Self {
            listen_addr,
            upstream_addr,
            socket: Arc::new(socket),
            sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            log_dir,
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Proxy server running on {}", self.listen_addr);

        let mut buf = vec![0u8; 65535]; // Max UDP packet size
        let socket = Arc::clone(&self.socket);
        let upstream_addr = self.upstream_addr;
        let sessions = Arc::clone(&self.sessions);

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((n, from_addr)) => {
                    let packet_data = buf[..n].to_vec();
                    
                    // Check if this packet is from a client or from upstream
                    let sessions_read = sessions.read().await;
                    let is_client = sessions_read.contains_key(&from_addr);
                    drop(sessions_read);

                    if is_client || from_addr != upstream_addr {
                        // This is a packet from a client
                        debug!("Received {} bytes from client {}", n, from_addr);
                        if let Err(e) = self.handle_client_packet(from_addr, packet_data).await {
                            error!("Error handling packet from {}: {}", from_addr, e);
                        }
                    } else {
                        // This is a packet from upstream server
                        debug!("Received {} bytes from upstream server {}", n, from_addr);
                        
                        // Find the session this packet belongs to
                        // TODO: In a real implementation, we'd need to:
                        // 1. Parse RakNet packet headers to identify the client
                        // 2. Use packet inspection or connection tracking
                        // 3. Or create one upstream socket per client session
                        // For now, forward to the first active session
                        // This works for single-client scenarios
                        
                        let sessions_read = sessions.read().await;
                        if sessions_read.is_empty() {
                            warn!("Received packet from upstream but no active sessions to forward to");
                        } else if let Some((client_addr, session)) = sessions_read.iter().next() {
                            // Log the clientbound packet
                            if let Err(e) = session.log_clientbound(packet_data.clone()).await {
                                error!("Failed to log clientbound packet: {}", e);
                            }
                            
                            // Forward to client
                            match socket.send_to(&packet_data, *client_addr).await {
                                Ok(_bytes_sent) => {
                                    debug!("Forwarded {} bytes to client {}", packet_data.len(), client_addr);
                                }
                                Err(e) => {
                                    error!("Failed to forward packet to client {}: {}", client_addr, e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving packet: {}", e);
                }
            }
        }
    }

    async fn handle_client_packet(&self, client_addr: SocketAddr, data: Vec<u8>) -> Result<()> {
        // Get or create session for this client
        let session = {
            let sessions = self.sessions.read().await;
            sessions.get(&client_addr).cloned()
        };

        let session = if let Some(session) = session {
            session
        } else {
            // Create new session
            let new_session = Arc::new(
                Session::new(client_addr, self.upstream_addr, &self.log_dir)
                    .context("Failed to create session")?
            );
            
            info!(
                "New session created: {} for client {}",
                new_session.id(),
                client_addr
            );

            // Store session
            {
                let mut sessions = self.sessions.write().await;
                sessions.insert(client_addr, new_session.clone());
            }

            new_session
        };

        // Log the serverbound packet
        session.log_serverbound(data.clone())
            .await
            .context("Failed to log packet")?;

        // Forward packet to upstream server
        match self.socket.send_to(&data, self.upstream_addr).await {
            Ok(_bytes_sent) => {
                debug!("Forwarded {} bytes to upstream server", data.len());
            }
            Err(e) => {
                error!("Failed to forward packet to upstream {}: {}", self.upstream_addr, e);
                return Err(anyhow::anyhow!(
                    "Failed to forward packet to upstream {}: {}",
                    self.upstream_addr,
                    e
                ));
            }
        }

        Ok(())
    }
}
