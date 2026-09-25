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

            // Initialize trade_records table matching Python's SQLAlchemy schema
            conn.execute(
                "CREATE TABLE IF NOT EXISTS trade_records (
                    id TEXT PRIMARY KEY,
                    client_order_id TEXT,
                    symbol TEXT,
                    side TEXT,
                    price REAL,
                    qty REAL,
                    value_asset REAL,
                    fee_asset REAL,
                    fx_rate_to_gbp REAL,
                    value_gbp REAL,
                    fee_gbp REAL,
                    realized_pnl_gbp REAL,
                    strategy_type TEXT,
                    execution_time DATETIME,
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
                )",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS ix_trade_records_symbol_exec ON trade_records (symbol, execution_time)",
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
                    ("BTC/USD", "BTC-USD", "BTC", "USD", 500.0, 0.0040, 5, 50.0, 0.012, 0, 50.0, 0.0011),
                    ("ETH/USD", "ETH-USD", "ETH", "USD", 500.0, 0.0040, 5, 50.0, 0.012, 0, 50.0, 0.0011),
                    ("SOL/USD", "SOL-USD", "SOL", "USD", 500.0, 0.0060, 5, 50.0, 0.015, 0, 50.0, 0.0011),
                    ("BTC/GBP", "BTC-GBP", "BTC", "GBP", 500.0, 0.0040, 5, 50.0, 0.012, 0, 50.0, 0.0011),
                    ("ETH/GBP", "ETH-GBP", "ETH", "GBP", 500.0, 0.0040, 5, 50.0, 0.012, 0, 50.0, 0.0011),
                    ("SOL/GBP", "SOL-GBP", "SOL", "GBP", 500.0, 0.0060, 5, 50.0, 0.015, 0, 50.0, 0.0011),
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
    }

    pub async fn update_order_status(&self, client_order_id: &str, status: &str) {
        let conn = self.conn.clone();
        let cid = client_order_id.to_string();
        let stat = status.to_string();
        
        let c = conn.lock().await;
        let res = c.execute(
            "UPDATE order_records SET status=?1 WHERE client_order_id=?2",
            params![stat, cid],
        );
        if let Err(e) = res {
            error!("Failed to update order {} status to DB: {}", cid, e);
        }
    }

    /// Reconstructs open inventory lots from historical trade records using strict FIFO matching.
    pub async fn reconstruct_open_lots(&self, symbol_slash: &str) -> anyhow::Result<Vec<crate::strategy::InventoryLot>> {
        let conn = self.conn.clone();
        let sym = symbol_slash.to_string();
        let c = conn.lock().await;

        let table_exists: i64 = c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='trade_records'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        if table_exists == 0 {
            return Ok(Vec::new());
        }

        let mut stmt = c.prepare(
            "SELECT id, client_order_id, side, price, qty
             FROM trade_records
             WHERE symbol = ?1
             ORDER BY execution_time ASC, created_at ASC"
        )?;

        let rows = stmt.query_map(params![sym], |row| {
            let id: String = row.get(0)?;
            let cid: Option<String> = row.get(1)?;
            let side: String = row.get(2)?;
            let price: f64 = row.get(3)?;
            let qty: f64 = row.get(4)?;
            Ok((id, cid.unwrap_or_default(), side, price, qty))
        })?;

        let mut open_lots: Vec<crate::strategy::InventoryLot> = Vec::new();
        let min_profit_margin = Decimal::from_str("0.0015").unwrap_or_default();
        let dust = Decimal::from_str("0.000001").unwrap_or_default();

        for r in rows {
            let (id_str, cid, side, price_f, qty_f) = r?;
            let price = Decimal::from_str(&format!("{:.8}", price_f)).unwrap_or_default();
            let qty = Decimal::from_str(&format!("{:.8}", qty_f)).unwrap_or_default();

            if price <= Decimal::ZERO || qty <= Decimal::ZERO {
                continue;
            }

            if side.eq_ignore_ascii_case("BUY") {
                let target_price = crate::strategy::round_price_for(price * (Decimal::ONE + min_profit_margin));
                let buy_order_uuid = uuid::Uuid::parse_str(&id_str).unwrap_or_else(|_| uuid::Uuid::new_v4());
                open_lots.push(crate::strategy::InventoryLot {
                    lot_id: uuid::Uuid::new_v4(),
                    buy_order_id: buy_order_uuid,
                    buy_client_order_id: cid,
                    buy_price: price,
                    qty,
                    counter_sell_order_id: None,
                    counter_sell_client_order_id: None,
                    target_sell_price: target_price,
                    is_closed: false,
                });
            } else if side.eq_ignore_ascii_case("SELL") {
                let mut needed = qty;
                for lot in open_lots.iter_mut() {
                    if !lot.is_closed {
                        let take = needed.min(lot.qty);
                        lot.qty -= take;
                        needed -= take;
                        if lot.qty <= dust {
                            lot.is_closed = true;
                        }
                        if needed <= dust {
                            break;
                        }
                    }
                }
            }
        }

        open_lots.retain(|l| !l.is_closed && l.qty > dust);
        info!(
            "[DB-LOT-RECOVERY] Reconstructed {} open lots for {} from trade_records (total qty: {})",
            open_lots.len(),
            sym,
            open_lots.iter().map(|l| l.qty).sum::<Decimal>()
        );
        Ok(open_lots)
    }

    /// Saves a trade fill record directly into SQLite trade_records table with exact realized PnL.
    pub async fn save_trade_fill(&self, fill: &crate::model::Fill, pnl: Decimal) {
        let conn = self.conn.clone();
        let fill_id = fill.order_id.to_string();
        let cid = fill.client_order_id.clone();
        let symbol = fill.symbol.as_slash();
        let side = fill.side.to_string();
        let price = fill.price.to_string().parse::<f64>().unwrap_or(0.0);
        let qty = fill.qty.to_string().parse::<f64>().unwrap_or(0.0);
        let val_asset = price * qty;
        let fee_asset = fill.fee.to_string().parse::<f64>().unwrap_or(0.0);
        let fx_rate = 1.0;
        let val_gbp = val_asset * fx_rate;
        let fee_gbp = fee_asset * fx_rate;
        let pnl_gbp = pnl.to_string().parse::<f64>().unwrap_or(0.0);
        let strat_type = "Maker Grid".to_string();
        let exec_time = fill.timestamp.to_rfc3339();

        let c = conn.lock().await;
        let res = c.execute(
            "INSERT INTO trade_records (
                id, client_order_id, symbol, side, price, qty,
                value_asset, fee_asset, fx_rate_to_gbp, value_gbp, fee_gbp,
                realized_pnl_gbp, strategy_type, execution_time
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(id) DO UPDATE SET realized_pnl_gbp=excluded.realized_pnl_gbp",
            params![
                fill_id, cid, symbol, side, price, qty,
                val_asset, fee_asset, fx_rate, val_gbp, fee_gbp,
                pnl_gbp, strat_type, exec_time
            ],
        );
        if let Err(e) = res {
            error!("Failed to save trade fill {} to DB: {}", cid, e);
        }
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
