use std::net::SocketAddr;
use uuid::Uuid;
use crate::packet_logger::{PacketLogger, PacketDirection};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Session {
    id: Uuid,
    client_addr: SocketAddr,
    upstream_addr: SocketAddr,
    logger: Arc<Mutex<PacketLogger>>,
}

impl Session {
    pub fn new(
        client_addr: SocketAddr,
        upstream_addr: SocketAddr,
        log_dir: impl AsRef<std::path::Path>,
    ) -> Result<Self, std::io::Error> {
        let id = Uuid::new_v4();
        let logger = PacketLogger::new(id, log_dir)?;
        
        Ok(Self {
            id,
            client_addr,
            upstream_addr,
            logger: Arc::new(Mutex::new(logger)),
        })
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn client_addr(&self) -> SocketAddr {
        self.client_addr
    }

    pub fn upstream_addr(&self) -> SocketAddr {
        self.upstream_addr
    }

    pub async fn log_clientbound(&self, data: Vec<u8>) -> Result<(), std::io::Error> {
        let mut logger = self.logger.lock().await;
        logger.log_packet(PacketDirection::Clientbound, data)
    }

    pub async fn log_serverbound(&self, data: Vec<u8>) -> Result<(), std::io::Error> {
        let mut logger = self.logger.lock().await;
        logger.log_packet(PacketDirection::Serverbound, data)
    }
}
