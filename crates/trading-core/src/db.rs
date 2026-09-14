use rusqlite::{params, Connection};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::model::{Order, PairConfig, Symbol};
use rust_decimal::Decimal;
use std::str::FromStr;
use tracing::{info, error};

#[derive(Clone)]
pub struct DbStore {
    conn: Arc<Mutex<Connection>>,
}

impl DbStore {
    pub async fn new(db_path: &str) -> anyhow::Result<Self> {
        let path = db_path.to_string();
        let conn = tokio::task::spawn_blocking(move || -> anyhow::Result<Connection> {
            let conn = Connection::open(&path)?;
            conn.execute_batch("
                PRAGMA journal_mode = WAL;
                PRAGMA busy_timeout = 5000;
                PRAGMA synchronous = NORMAL;
            ")?;
            // Initialize tables matching Python's SQLAlchemy schema
            conn.execute(
                "CREATE TABLE IF NOT EXISTS order_records (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    client_order_id TEXT UNIQUE,
                    runner_id TEXT,
                    symbol TEXT,
                    side TEXT,
                    price REAL,
                    qty REAL,
                    status TEXT,
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
                )",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS ix_order_records_client_order_id ON order_records (client_order_id)",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS ix_order_records_runner_id ON order_records (runner_id)",
                [],
            )?;

            // Initialize pair_configurations table
            conn.execute(
                "CREATE TABLE IF NOT EXISTS pair_configurations (
                    symbol TEXT PRIMARY KEY,
                    venue_symbol TEXT NOT NULL,
                    base_asset TEXT NOT NULL,
                    quote_asset TEXT NOT NULL,
                    envelope_capital REAL NOT NULL,
                    grid_step_pct REAL NOT NULL,
                    grid_rungs INTEGER NOT NULL,
                    order_size_fiat REAL NOT NULL,
                    rebalance_threshold_pct REAL NOT NULL,
                    sniper_enabled INTEGER NOT NULL DEFAULT 1,
                    sniper_order_size_fiat REAL NOT NULL DEFAULT 50.0,
                    sniper_hurdle_pct REAL NOT NULL DEFAULT 0.0011,
                    is_active INTEGER NOT NULL DEFAULT 1,
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
                )",
                [],
            )?;

            // Seed default trading pairs if empty
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM pair_configurations",
                [],
                |row| row.get(0),
            ).unwrap_or(0);

            if count == 0 {
                let default_pairs = vec![
                    ("BTC/USD", "BTC-USD", "BTC", "USD", 500.0, 0.0040, 5, 50.0, 0.012, 1, 50.0, 0.0011),
                    ("ETH/USD", "ETH-USD", "ETH", "USD", 500.0, 0.0040, 5, 50.0, 0.012, 1, 50.0, 0.0011),
                    ("SOL/USD", "SOL-USD", "SOL", "USD", 500.0, 0.0060, 5, 50.0, 0.015, 1, 50.0, 0.0011),
                    ("BTC/GBP", "BTC-GBP", "BTC", "GBP", 500.0, 0.0040, 5, 50.0, 0.012, 1, 50.0, 0.0011),
                    ("ETH/GBP", "ETH-GBP", "ETH", "GBP", 500.0, 0.0040, 5, 50.0, 0.012, 1, 50.0, 0.0011),
                    ("SOL/GBP", "SOL-GBP", "SOL", "GBP", 500.0, 0.0060, 5, 50.0, 0.015, 1, 50.0, 0.0011),
                ];
                for p in default_pairs {
                    let _ = conn.execute(
                        "INSERT INTO pair_configurations (
                            symbol, venue_symbol, base_asset, quote_asset,
                            envelope_capital, grid_step_pct, grid_rungs, order_size_fiat,
                            rebalance_threshold_pct, sniper_enabled, sniper_order_size_fiat,
                            sniper_hurdle_pct, is_active
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1)",
                        params![p.0, p.1, p.2, p.3, p.4, p.5, p.6, p.7, p.8, p.9, p.10, p.11],
                    );
                }
                info!("Seeded default trading pairs into pair_configurations table");
            }

            Ok(conn)
        })
        .await??;

        info!("SQLite database initialized at {}", db_path);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn save_order(&self, order: &Order, status: &str) {
        let conn = self.conn.clone();
        let status = status.to_string();
        let client_order_id = order.client_order_id.clone();
        let runner_id = order.runner_id.clone();
        let symbol = order.symbol.as_dash();
        let side = order.side.to_string();
        let price = order.price.to_string().parse::<f64>().unwrap_or(0.0);
        let qty = order.qty.to_string().parse::<f64>().unwrap_or(0.0);
        
        tokio::spawn(async move {
            let c = conn.lock().await;
            let res = c.execute(
                "INSERT INTO order_records (client_order_id, runner_id, symbol, side, price, qty, status) 
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(client_order_id) DO UPDATE SET status=excluded.status",
                params![
                    client_order_id,
                    runner_id,
                    symbol,
                    side,
                    price,
                    qty,
                    status,
                ],
            );
            if let Err(e) = res {
                error!("Failed to save order {} to DB: {}", client_order_id, e);
            }
        });
    }

    pub async fn update_order_status(&self, client_order_id: &str, status: &str) {
        let conn = self.conn.clone();
        let cid = client_order_id.to_string();
        let stat = status.to_string();
        
        tokio::spawn(async move {
            let c = conn.lock().await;
            let res = c.execute(
                "UPDATE order_records SET status=?1 WHERE client_order_id=?2",
                params![stat, cid],
            );
            if let Err(e) = res {
                error!("Failed to update order {} status to DB: {}", cid, e);
            }
        });
    }

    pub async fn get_active_pair_configs(&self) -> anyhow::Result<Vec<PairConfig>> {
        let conn = self.conn.clone();
        let c = conn.lock().await;
        let mut stmt = c.prepare(
            "SELECT symbol, base_asset, quote_asset, envelope_capital, grid_step_pct,
                    grid_rungs, order_size_fiat, rebalance_threshold_pct,
                    sniper_enabled, sniper_order_size_fiat, sniper_hurdle_pct, is_active
             FROM pair_configurations
             WHERE is_active = 1",
        )?;

        let rows = stmt.query_map([], |row| {
            let sym_str: String = row.get(0)?;
            let base: String = row.get(1)?;
            let quote: String = row.get(2)?;
            let env_cap: f64 = row.get(3)?;
            let grid_step: f64 = row.get(4)?;
            let rungs: i64 = row.get(5)?;
            let size_fiat: f64 = row.get(6)?;
            let rebal_pct: f64 = row.get(7)?;
            let sniper_en: i64 = row.get(8)?;
            let sniper_sz: f64 = row.get(9)?;
            let sniper_hrd: f64 = row.get(10)?;
            let active: i64 = row.get(11)?;

            let symbol = Symbol::parse(&sym_str).unwrap_or_else(|| Symbol::new(base, quote));
            let to_dec = |v: f64| Decimal::from_str(&format!("{:.8}", v)).unwrap_or_default();

            Ok(PairConfig {
                symbol,
                envelope_capital: to_dec(env_cap),
                grid_step_pct: to_dec(grid_step),
                grid_rungs: rungs as usize,
                order_size_fiat: to_dec(size_fiat),
                rebalance_threshold_pct: to_dec(rebal_pct),
                sniper_enabled: sniper_en != 0,
                sniper_order_size_fiat: to_dec(sniper_sz),
                sniper_hurdle_pct: to_dec(sniper_hrd),
                is_active: active != 0,
            })
        })?;

        let mut configs = Vec::new();
        for r in rows {
            configs.push(r?);
        }
        Ok(configs)
    }

    pub async fn upsert_pair_config(&self, config: &PairConfig) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let c = conn.lock().await;
        let sym_slash = config.symbol.as_slash();
        let sym_dash = config.symbol.as_dash();
        let base = config.symbol.base.clone();
        let quote = config.symbol.quote.clone();
        let env_cap = config.envelope_capital.to_string().parse::<f64>().unwrap_or(500.0);
        let step_pct = config.grid_step_pct.to_string().parse::<f64>().unwrap_or(0.004);
        let rungs = config.grid_rungs as i64;
        let ord_size = config.order_size_fiat.to_string().parse::<f64>().unwrap_or(50.0);
        let rebal_pct = config.rebalance_threshold_pct.to_string().parse::<f64>().unwrap_or(0.012);
        let sniper_en = if config.sniper_enabled { 1 } else { 0 };
        let sniper_sz = config.sniper_order_size_fiat.to_string().parse::<f64>().unwrap_or(50.0);
        let sniper_hrd = config.sniper_hurdle_pct.to_string().parse::<f64>().unwrap_or(0.0011);
        let is_act = if config.is_active { 1 } else { 0 };

        c.execute(
            "INSERT INTO pair_configurations (
                symbol, venue_symbol, base_asset, quote_asset, envelope_capital,
                grid_step_pct, grid_rungs, order_size_fiat, rebalance_threshold_pct,
                sniper_enabled, sniper_order_size_fiat, sniper_hurdle_pct, is_active
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(symbol) DO UPDATE SET
                envelope_capital=excluded.envelope_capital,
                grid_step_pct=excluded.grid_step_pct,
                grid_rungs=excluded.grid_rungs,
                order_size_fiat=excluded.order_size_fiat,
                rebalance_threshold_pct=excluded.rebalance_threshold_pct,
                sniper_enabled=excluded.sniper_enabled,
                sniper_order_size_fiat=excluded.sniper_order_size_fiat,
                sniper_hurdle_pct=excluded.sniper_hurdle_pct,
                is_active=excluded.is_active",
            params![
                sym_slash, sym_dash, base, quote, env_cap, step_pct, rungs,
                ord_size, rebal_pct, sniper_en, sniper_sz, sniper_hrd, is_act
            ],
        )?;

        info!("Upserted pair configuration for {}", sym_slash);
        Ok(())
    }

    pub async fn toggle_pair_active(&self, symbol: &str, is_active: bool) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let c = conn.lock().await;
        let act = if is_active { 1 } else { 0 };
        c.execute(
            "UPDATE pair_configurations SET is_active = ?1 WHERE symbol = ?2 OR venue_symbol = ?2",
            params![act, symbol],
        )?;
        Ok(())
    }

    pub async fn delete_pair_config(&self, symbol: &str) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let c = conn.lock().await;
        c.execute(
            "DELETE FROM pair_configurations WHERE symbol = ?1 OR venue_symbol = ?1",
            params![symbol],
        )?;
        info!("Deleted pair configuration for {}", symbol);
        Ok(())
    }
}
