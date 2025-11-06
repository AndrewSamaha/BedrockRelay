use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio_postgres::{Client, NoTls, types::Json};

pub struct Database {
    client: Client,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i32,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct DbPacket {
    pub id: i32,
    pub session_id: i32,
    pub ts: DateTime<Utc>,
    pub session_time_ms: i64,
    pub packet_number: i64,
    pub server_version: String,
    pub direction: String,
    pub packet: Value,
}

impl Database {
    pub async fn connect() -> Result<Self> {
        // Get connection string from environment variables
        let host = std::env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = std::env::var("DB_PORT")
            .unwrap_or_else(|_| "5432".to_string())
            .parse::<u16>()
            .context("Invalid DB_PORT")?;
        let user = std::env::var("DB_USER").unwrap_or_else(|_| "postgres".to_string());
        let password = std::env::var("DB_PASSWORD").unwrap_or_else(|_| "postgres".to_string());
        let dbname = std::env::var("DB_NAME").unwrap_or_else(|_| "postgres".to_string());

        let connection_string = format!(
            "host={} port={} user={} password={} dbname={}",
            host, port, user, password, dbname
        );

        let (client, connection) = tokio_postgres::connect(&connection_string, NoTls)
            .await
            .with_context(|| format!(
                "Failed to connect to database at {}:{} (user: {}, db: {}). \
                Make sure your .env file is loaded and contains DB_HOST, DB_PORT, DB_USER, DB_PASSWORD, and DB_NAME",
                host, port, user, dbname
            ))?;

        // Spawn connection task
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Database connection error: {}", e);
            }
        });

        Ok(Self { client })
    }

    pub async fn get_sessions(&self) -> Result<Vec<Session>> {
        let rows = self
            .client
            .query(
                "SELECT id, started_at, ended_at FROM sessions ORDER BY started_at DESC",
                &[],
            )
            .await
            .context("Failed to query sessions")?;

        let mut sessions = Vec::new();
        for row in rows {
            // PostgreSQL TIMESTAMP is read as NaiveDateTime, then convert to DateTime<Utc>
            let started_at_naive: chrono::NaiveDateTime = row.get(1);
            let ended_at_naive: Option<chrono::NaiveDateTime> = row.get(2);
            
            sessions.push(Session {
                id: row.get(0),
                started_at: DateTime::from_naive_utc_and_offset(started_at_naive, Utc),
                ended_at: ended_at_naive.map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc)),
            });
        }

        Ok(sessions)
    }

    pub async fn get_session_packet_count(&self, session_id: i32) -> Result<usize> {
        let row = self
            .client
            .query_one(
                "SELECT COUNT(*) FROM packets WHERE session_id = $1",
                &[&session_id],
            )
            .await
            .context("Failed to count packets")?;

        Ok(row.get::<_, i64>(0) as usize)
    }

    pub async fn get_packets(&self, session_id: i32, direction_filter: Option<u8>) -> Result<Vec<DbPacket>> {
        // direction_filter: 0 = clientbound, 1 = serverbound, None = all
        let rows = match direction_filter {
            Some(0) => {
                self.client
                    .query(
                        "SELECT id, session_id, ts, session_time_ms, packet_number, server_version, direction, packet 
                     FROM packets 
                     WHERE session_id = $1 AND direction = 'clientbound'
                     ORDER BY packet_number ASC",
                        &[&session_id],
                    )
                    .await
            }
            Some(1) => {
                self.client
                    .query(
                        "SELECT id, session_id, ts, session_time_ms, packet_number, server_version, direction, packet 
                     FROM packets 
                     WHERE session_id = $1 AND direction = 'serverbound'
                     ORDER BY packet_number ASC",
                        &[&session_id],
                    )
                    .await
            }
            Some(_) | None => {
                // Invalid filter value or no filter - show all packets
                self.client
                    .query(
                        "SELECT id, session_id, ts, session_time_ms, packet_number, server_version, direction, packet 
                     FROM packets 
                     WHERE session_id = $1 
                     ORDER BY packet_number ASC",
                        &[&session_id],
                    )
                    .await
            }
        }
        .context("Failed to query packets")?;

        let mut packets = Vec::new();
        for row in rows {
            // PostgreSQL TIMESTAMP is read as NaiveDateTime, then convert to DateTime<Utc>
            let ts_naive: chrono::NaiveDateTime = row.get(2);
            let packet_json: Json<Value> = row.get(7);
            
            packets.push(DbPacket {
                id: row.get(0),
                session_id: row.get(1),
                ts: DateTime::from_naive_utc_and_offset(ts_naive, Utc),
                session_time_ms: row.get(3),
                packet_number: row.get(4),
                server_version: row.get(5),
                direction: row.get(6),
                packet: packet_json.0,
            });
        }

        Ok(packets)
    }
}
