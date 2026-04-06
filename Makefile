# Makefile for led-server service management on Raspberry Pi.
#
# Run on the RPi:
#   sudo make install    — install and enable the systemd service
#   sudo make uninstall  — stop, disable and remove the service
#   sudo make start / stop / restart / status
#
# Run on macOS:
#   make deploy     — sync source and build on RPi (calls deploy.sh)

SERVICE_NAME  := led-server
SERVICE_FILE  := /etc/systemd/system/$(SERVICE_NAME).service
BINARY        := $(shell pwd)/target/release/$(SERVICE_NAME)
ASSETS        := $(shell pwd)/assets

# Panel configuration
PANEL_ROWS           ?= 32
PANEL_COLS           ?= 64
PANEL_BRIGHTNESS     ?= 80
PANEL_REFRESH_RATE   ?= 60
PANEL_SLOWDOWN       ?= 2

# Optional features (leave empty to disable)
EYECATCH_PATH        ?= $(ASSETS)/butacowalk2.gif
EYECATCH_DURATION_MS ?= 5000
JINGLE_PATH          ?= $(ASSETS)/splanews.wav

# gRPC
GRPC_ADDR ?= 0.0.0.0:50051

CARGO ?= $(HOME)/.cargo/bin/cargo

RUST_LOG   ?= info
LOG_FORMAT ?=

OS := $(shell uname -s)

.PHONY: help build install uninstall start stop restart status deploy run assert-rpi assert-macos

help:
	@echo "Usage: make <target>"
	@echo ""
	@echo "Targets for Raspberry Pi:"
	@echo "  install    Install and enable the systemd service"
	@echo "  uninstall  Stop, disable and remove the service"
	@echo "  start      Start the service"
	@echo "  stop       Stop the service"
	@echo "  restart    Restart the service"
	@echo "  status     Show service status"
	@echo ""
	@echo "Targets for macOS:"
	@echo "  run        Run the server (emulator on macOS, rpi binary on RPi)"
	@echo ""
	@echo "Common targets:"
	@echo "  build      Build the server (emulator on macOS, rpi feature on RPi)"
	@echo "  deploy     Sync source to RPi and build there"

assert-rpi:
	@[ "$(OS)" = "Linux" ] || { echo "Error: '$(MAKECMDGOALS)' is only supported on Raspberry Pi (Linux)."; exit 1; }

assert-macos:
	@[ "$(OS)" = "Darwin" ] || { echo "Error: '$(MAKECMDGOALS)' is only supported on macOS."; exit 1; }

install: assert-rpi
	@echo "Creating $(SERVICE_FILE) ..."
	@{ \
		echo '[Unit]'; \
		echo 'Description=LED Panel Service'; \
		echo 'After=network.target'; \
		echo ''; \
		echo '[Service]'; \
		echo 'Type=simple'; \
		echo 'User=root'; \
		echo "WorkingDirectory=$(shell pwd)"; \
		echo "ExecStart=$(BINARY)"; \
		echo 'Restart=on-failure'; \
		echo 'RestartSec=5'; \
		echo ''; \
		echo "Environment=LOG_FORMAT=json"; \
		echo "Environment=GRPC_ADDR=$(GRPC_ADDR)"; \
		echo "Environment=PANEL_ROWS=$(PANEL_ROWS)"; \
		echo "Environment=PANEL_COLS=$(PANEL_COLS)"; \
		echo "Environment=PANEL_BRIGHTNESS=$(PANEL_BRIGHTNESS)"; \
		echo "Environment=PANEL_REFRESH_RATE=$(PANEL_REFRESH_RATE)"; \
		echo "Environment=PANEL_SLOWDOWN=$(PANEL_SLOWDOWN)"; \
		echo "Environment=EYECATCH_PATH=$(EYECATCH_PATH)"; \
		echo "Environment=EYECATCH_DURATION_MS=$(EYECATCH_DURATION_MS)"; \
		echo "Environment=JINGLE_PATH=$(JINGLE_PATH)"; \
		echo ''; \
		echo '[Install]'; \
		echo 'WantedBy=multi-user.target'; \
	} > $(SERVICE_FILE)
	systemctl daemon-reload
	systemctl enable $(SERVICE_NAME)
	systemctl start $(SERVICE_NAME)
	@echo "Service $(SERVICE_NAME) installed and started."

uninstall: assert-rpi
	systemctl stop $(SERVICE_NAME) || true
	systemctl disable $(SERVICE_NAME) || true
	rm -f $(SERVICE_FILE)
	systemctl daemon-reload
	@echo "Service $(SERVICE_NAME) removed."

start: assert-rpi
	systemctl start $(SERVICE_NAME)

stop: assert-rpi
	systemctl stop $(SERVICE_NAME)

restart: assert-rpi
	systemctl restart $(SERVICE_NAME)

status: assert-rpi
	systemctl status $(SERVICE_NAME)

# Run from macOS: sync source and build on RPi
deploy:
	./deploy.sh

# Build: emulator on macOS, rpi feature on RPi
ifeq ($(OS),Darwin)
build:
	$(CARGO) build --bin led-server
else
build:
	$(CARGO) build --release --bin led-server --features rpi --no-default-features
endif

# Run: emulator on macOS, rpi binary on RPi
ifeq ($(OS),Darwin)
run:
	GRPC_ADDR=$(GRPC_ADDR) \
	PANEL_ROWS=$(PANEL_ROWS) \
	PANEL_COLS=$(PANEL_COLS) \
	EYECATCH_PATH=$(EYECATCH_PATH) \
	EYECATCH_DURATION_MS=$(EYECATCH_DURATION_MS) \
	JINGLE_PATH=$(JINGLE_PATH) \
	$(CARGO) run --bin led-server
else
run:
	sudo \
	RUST_LOG=$(RUST_LOG) \
	LOG_FORMAT=$(LOG_FORMAT) \
	GRPC_ADDR=$(GRPC_ADDR) \
	PANEL_ROWS=$(PANEL_ROWS) \
	PANEL_COLS=$(PANEL_COLS) \
	PANEL_BRIGHTNESS=$(PANEL_BRIGHTNESS) \
	PANEL_REFRESH_RATE=$(PANEL_REFRESH_RATE) \
	PANEL_SLOWDOWN=$(PANEL_SLOWDOWN) \
	EYECATCH_PATH=$(EYECATCH_PATH) \
	EYECATCH_DURATION_MS=$(EYECATCH_DURATION_MS) \
	JINGLE_PATH=$(JINGLE_PATH) \
	./target/release/led-server
endif
