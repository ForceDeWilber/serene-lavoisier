use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Token-bucket rate limiter ensuring we stay comfortably under the 1000 requests/minute ceiling
#[derive(Clone)]
pub struct TokenBucketRateLimiter {
    state: Arc<Mutex<RateLimiterState>>,
}

struct RateLimiterState {
    capacity: f64,
    tokens: f64,
    refill_rate_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucketRateLimiter {
    /// E.g. capacity = 30.0, max_req_per_minute = 900.0 (15.0 tokens/sec)
    pub fn new(capacity: f64, max_req_per_minute: f64) -> Self {
        let refill_rate_per_sec = max_req_per_minute / 60.0;
        Self {
            state: Arc::new(Mutex::new(RateLimiterState {
                capacity,
                tokens: capacity,
                refill_rate_per_sec,
                last_refill: Instant::now(),
            })),
        }
    }

    /// Acquires a token, asynchronously waiting if the bucket is empty
    pub async fn acquire(&self) {
        loop {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            let elapsed = now.duration_since(state.last_refill).as_secs_f64();
            state.tokens = (state.tokens + elapsed * state.refill_rate_per_sec).min(state.capacity);
            state.last_refill = now;

            if state.tokens >= 1.0 {
                state.tokens -= 1.0;
                return;
            }

            // Calculate required wait time for next token
            let deficit = 1.0 - state.tokens;
            let wait_secs = deficit / state.refill_rate_per_sec;
            tracing::warn!(
                "[RATE-LIMIT] Revolut X token bucket depleted (tokens: {:.2}/{:.0}). Throttling request for {:.1}ms",
                state.tokens, state.capacity, wait_secs * 1000.0
            );
            drop(state);

            tokio::time::sleep(Duration::from_secs_f64(wait_secs)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_acquire() {
        let limiter = TokenBucketRateLimiter::new(2.0, 60.0); // 1 token / sec
        limiter.acquire().await;
        limiter.acquire().await;
        // Should acquire 2 tokens without blocking
    }
}
