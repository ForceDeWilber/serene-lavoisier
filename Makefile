.PHONY: all engine backend frontend test clean run-all

all: build

build:
	cargo build --workspace
	cd frontend && npm run build

test:
	cargo test --workspace

engine:
	cargo run -p engine-daemon

backend:
	PYTHONPATH=backend ./backend/.venv/bin/python backend/run.py

frontend:
	npm run dev --prefix frontend

clean:
	cargo clean
	rm -rf trading.db /tmp/trading_engine.sock frontend/.next
