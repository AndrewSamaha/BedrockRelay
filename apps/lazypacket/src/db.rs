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

#[derive(Debug, Clone)]
pub struct DbPacketFilter {
    pub direction: Option<String>, // "clientbound", "serverbound", or None for all
    pub packet_name: Option<String>, // Packet name to filter by, or None for all
    pub packet_name_is_wildcard: bool, // If true, use ILIKE with wildcards; if false, use exact match
}

#[derive(Debug, Clone)]
pub struct DbPacketFilterSet {
    pub filters: Vec<DbPacketFilter>, // OR logic: packet matches if it matches any filter
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

    pub async fn get_packets(&self, session_id: i32, filter_set: Option<&DbPacketFilterSet>) -> Result<Vec<DbPacket>> {
        let rows = if let Some(filter_set) = filter_set {
            if filter_set.filters.is_empty() {
                // No filters - show all packets
                self.client
                    .query(
                        "SELECT id, session_id, ts, session_time_ms, packet_number, server_version, direction, packet 
                     FROM packets 
                     WHERE session_id = $1 
                     ORDER BY packet_number ASC",
                        &[&session_id],
                    )
                    .await
            } else {
                // Build WHERE clause with OR conditions for each filter
                let mut conditions = Vec::new();
                let mut param_index = 1;
                let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = vec![Box::new(session_id)];
                
                for filter in &filter_set.filters {
                    let mut filter_conditions = Vec::new();
                    
                    // Direction filter
                    if let Some(ref direction) = filter.direction {
                        filter_conditions.push(format!("direction = '{}'", direction));
                    }
                    
                    // Packet name filter
                    if let Some(ref packet_name) = filter.packet_name {
                        param_index += 1;
                        if filter.packet_name_is_wildcard {
                            // Convert * to % for SQL ILIKE pattern matching
                            // Note: Users can include literal % or _ in their pattern if needed by escaping
                            let sql_pattern = packet_name.replace('*', "%");
                            
                            filter_conditions.push(format!("packet->>'name' ILIKE ${}", param_index));
                            params.push(Box::new(sql_pattern));
                        } else {
                            // Exact match
                            filter_conditions.push(format!("packet->>'name' = ${}", param_index));
                            params.push(Box::new(packet_name.clone()));
                        }
                    }
                    
                    // Combine conditions for this filter with AND
                    if !filter_conditions.is_empty() {
                        conditions.push(format!("({})", filter_conditions.join(" AND ")));
                    } else {
                        // No conditions means match all - but we still need a condition
                        // This shouldn't happen in practice, but handle it
                        conditions.push("1=1".to_string());
                    }
                }
                
                // Combine all filters with OR
                let where_clause = if conditions.is_empty() {
                    "session_id = $1".to_string()
                } else {
                    format!("session_id = $1 AND ({})", conditions.join(" OR "))
                };
                
                let query = format!(
                    "SELECT id, session_id, ts, session_time_ms, packet_number, server_version, direction, packet 
                     FROM packets 
                     WHERE {}
                     ORDER BY packet_number ASC",
                    where_clause
                );
                
                // Convert Vec<Box<dyn ToSql + Sync>> to &[&dyn ToSql + Sync]
                let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
                self.client.query(&query, &param_refs[..]).await
            }
        } else {
            // No filter set - show all packets
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
